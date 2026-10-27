"""Procedurally builds burn_animals.blend: one scene per burn animal.

Run (from this directory):
    <blender> --background --factory-startup --python-exit-code 1 \
        --python build_scene.py -- --output burn_animals.blend

Each scene is named by the animal's stable asset id and holds a joint rig
(a hierarchy of empties), body parts parented to the joints, and one action
per joint keyed on every frame 0..24 (frame 24 == frame 0, so the loop is
seamless). Poses come from closed-form periodic functions of the loop phase;
legs use two-bone IK against a fixed ground line, so feet in stance stay on the
ground and slide back at one shared treadmill speed (running in place).

Conventions: the animal faces +X, Z is up, the camera looks along +Y, so the
near side is -Y. Angles are counter-clockwise as seen by the camera, in
radians. A limb segment hangs along its joint's local -Z, so a segment that
points in world direction phi has joint world rotation phi + pi/2.

render.py renders the saved .blend; it never imports this script.
"""

import math
import os
import sys

import bmesh
import bpy
from mathutils import Matrix

sys.dont_write_bytecode = True  # keep __pycache__ out of the source tree
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import common as C  # noqa: E402

TAU = 2.0 * math.pi
HALF_PI = 0.5 * math.pi
METABALL_VISIBLE = 0.575  # an isolated metaball's visible radius per unit of radius


# ---------------------------------------------------------------- math helpers

def wrap(x):
    return x - math.floor(x)


def rot(v, a):
    c, s = math.cos(a), math.sin(a)
    return (v[0] * c - v[1] * s, v[0] * s + v[1] * c)


def add(a, b):
    return (a[0] + b[0], a[1] + b[1])


def wave(p, amp, at):
    """Cosine with period 1 that peaks (value amp) at phase `at`."""
    return amp * math.cos(TAU * (p - at))


def loop_curve(points):
    """Periodic Catmull-Rom curve through (phase, value) points, phase in [0, 1)."""
    pts = sorted(points)
    n = len(pts)

    def at(i):
        k, extra = i % n, (i // n)
        return pts[k][0] + extra, pts[k][1]

    def f(p):
        p = wrap(p)
        i = 0
        while i < n - 1 and pts[i + 1][0] <= p:
            i += 1
        if p < pts[0][0]:
            i = -1
        x0, y0 = at(i - 1)
        x1, y1 = at(i)
        x2, y2 = at(i + 1)
        x3, y3 = at(i + 2)
        span = x2 - x1
        t = (p - x1) / span
        m1 = (y2 - y0) / (x2 - x0) * span
        m2 = (y3 - y1) / (x3 - x1) * span
        t2, t3 = t * t, t * t * t
        return ((2 * t3 - 3 * t2 + 1) * y1 + (t3 - 2 * t2 + t) * m1
                + (-2 * t3 + 3 * t2) * y2 + (t3 - t2) * m2)

    return f


def path_curve(points):
    """Catmull-Rom curve through (u, value) points for u in [0, 1] (ends clamped)."""
    pts = sorted(points)
    xs = [x for x, _ in pts]
    ys = [y for _, y in pts]

    def f(u):
        u = min(max(u, xs[0]), xs[-1])
        i = 0
        while i < len(xs) - 2 and xs[i + 1] <= u:
            i += 1
        y0 = ys[max(i - 1, 0)]
        y3 = ys[min(i + 2, len(ys) - 1)]
        x0 = xs[max(i - 1, 0)]
        x3 = xs[min(i + 2, len(xs) - 1)]
        x1, x2, y1, y2 = xs[i], xs[i + 1], ys[i], ys[i + 1]
        span = x2 - x1
        t = (u - x1) / span
        m1 = (y2 - y0) / (x2 - x0) * span if x2 > x0 else 0.0
        m2 = (y3 - y1) / (x3 - x1) * span if x3 > x1 else 0.0
        t2, t3 = t * t, t * t * t
        return ((2 * t3 - 3 * t2 + 1) * y1 + (t3 - 2 * t2 + t) * m1
                + (-2 * t3 + 3 * t2) * y2 + (t3 - t2) * m2)

    return f


def foot_path(q, stance, reach, lift, ground, cx, peak=0.7):
    """Foot position for leg phase q (0 = touchdown).

    Stance: the foot is on the ground and slides from cx+reach to cx-reach at
    constant speed (the treadmill). Swing: it eases forward along an arc whose
    height peaks early (peak < 1 tucks the foot right after lift-off).
    Returns (x, z, swing_u) where swing_u is -1 in stance, else 0..1.
    """
    if q < stance:
        u = q / stance
        return cx + reach * (1.0 - 2.0 * u), ground, -1.0
    u = (q - stance) / (1.0 - stance)
    x = cx - reach * math.cos(math.pi * u)
    z = ground + lift * math.sin(math.pi * (u ** peak))
    return x, z, u


def srgb_to_linear(hex_color):
    h = hex_color.lstrip("#")
    out = []
    for i in (0, 2, 4):
        c = int(h[i:i + 2], 16) / 255.0
        out.append(c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4)
    return out


# ---------------------------------------------------------------- rig

class Joint:
    """A rig joint: an empty whose pose is an offset delta `d` and an angle `a`."""

    def __init__(self, rig, name, parent, offset, y=0.0, rest=0.0):
        self.rig, self.name, self.parent = rig, name, parent
        self.offset, self.y, self.rest = offset, y, rest
        self.d, self.a = (0.0, 0.0), rest
        ob = bpy.data.objects.new(f"{rig.aid}_{name}", None)
        ob.empty_display_size = 0.05
        ob.rotation_mode = "XYZ"
        rig.link(ob)
        ob.parent = parent.obj if parent else rig.fit
        self.obj = ob
        rig.joints.append(self)

    def world(self):
        """(position, angle) in the pre-fit animal frame."""
        o = add(self.offset, self.d)
        if self.parent is None:
            return o, self.a
        pp, pa = self.parent.world()
        return add(pp, rot(o, pa)), pa + self.a

    def parent_angle(self):
        return self.parent.world()[1] if self.parent else 0.0

    def set_world_angle(self, angle):
        self.a = angle - self.parent_angle()

    def key(self, frame):
        ob = self.obj
        ob.location = (self.offset[0] + self.d[0], self.y, self.offset[1] + self.d[1])
        ob.rotation_euler = (0.0, -self.a, 0.0)
        ob.keyframe_insert("location", frame=frame)
        ob.keyframe_insert("rotation_euler", index=1, frame=frame)


class Leg:
    """Two-bone leg (upper, lower) plus an end piece (hoof, paw or foot).

    bend=+1 puts the middle joint's apex forward (fore knee/carpus, stifle),
    bend=-1 puts it backward (hock).
    """

    def __init__(self, rig, name, parent, hip, y, l1, l2, bend):
        self.l1, self.l2, self.bend = l1, l2, bend
        self.upper = Joint(rig, name + "_upper", parent, hip, y=y)
        self.lower = Joint(rig, name + "_lower", self.upper, (0.0, -l1))
        self.end = Joint(rig, name + "_end", self.lower, (0.0, -l2))

    def reach(self, target):
        hip = self.upper.world()[0]  # its own angle does not move its origin
        pa = self.upper.parent_angle()
        dx, dz = target[0] - hip[0], target[1] - hip[1]
        dist = math.hypot(dx, dz)
        lo, hi = abs(self.l1 - self.l2) + 1e-4, self.l1 + self.l2 - 1e-4
        dist_c = min(max(dist, lo), hi)
        theta = math.atan2(dz, dx)
        cos_a = (self.l1 ** 2 + dist_c ** 2 - self.l2 ** 2) / (2.0 * self.l1 * dist_c)
        alpha = math.acos(max(-1.0, min(1.0, cos_a)))
        phi1 = theta + self.bend * alpha
        knee = (hip[0] + self.l1 * math.cos(phi1), hip[1] + self.l1 * math.sin(phi1))
        end = (hip[0] + dist_c * math.cos(theta), hip[1] + dist_c * math.sin(theta))
        phi2 = math.atan2(end[1] - knee[1], end[0] - knee[0])
        self.upper.a = (phi1 + HALF_PI) - pa
        self.lower.a = phi2 - phi1
        return phi2

    def end_direction(self, phi):
        """Point the end piece in world direction phi."""
        self.end.set_world_angle(phi + HALF_PI)


class Rig:
    def __init__(self, scene, aid, cycles):
        self.scene, self.aid, self.cycles = scene, aid, cycles
        self.joints = []
        self.fit = bpy.data.objects.new(f"{aid}_fit", None)
        self.link(self.fit)
        self.mats = {}

    def link(self, ob):
        self.scene.collection.objects.link(ob)

    # materials -----------------------------------------------------------
    def mat(self, key, hex_color, roughness=0.55):
        if key not in self.mats:
            m = bpy.data.materials.new(f"{self.aid}_{key}")
            m.diffuse_color = (*srgb_to_linear(hex_color), 1.0)
            m.roughness = roughness
            m.metallic = 0.0
            self.mats[key] = m
        return self.mats[key]

    # parts -----------------------------------------------------------------
    def blob(self, family, mat, joint, at, vis, size=(1.0, 1.0, 1.0), angle=0.0, y=0.0,
             kind="ELLIPSOID"):
        """A metaball element; elements of one family melt into one surface.

        vis is the visible radius of the element when isolated; size scales
        its local X (along `angle`), Y (depth) and Z axes.
        """
        name = f"{self.aid}{family}"
        mb = bpy.data.metaballs.new(name)
        mb.resolution = 0.03
        mb.render_resolution = 0.012
        mb.threshold = 0.6
        mb.materials.append(mat)
        ob = bpy.data.objects.new(name, mb)  # the first of a family is its basis
        el = mb.elements.new(type=kind)
        el.radius = vis / METABALL_VISIBLE
        el.size_x, el.size_y, el.size_z = size
        ob.location = (at[0], y, at[1])
        ob.rotation_euler = (0.0, -angle, 0.0)
        self.link(ob)
        ob.parent = joint.obj
        return ob

    def segment(self, mat, joint, a, b, r1, r2, y=0.0, name="seg"):
        """A tapered capsule mesh from local point a (radius r1) to b (radius r2)."""
        length = math.hypot(b[0] - a[0], b[1] - a[1])
        bm = bmesh.new()
        bmesh.ops.create_cone(bm, cap_ends=False, segments=20, radius1=r2, radius2=r1,
                              depth=length, matrix=Matrix.Translation((0, 0, -length / 2)))
        bmesh.ops.create_uvsphere(bm, u_segments=20, v_segments=10, radius=r1)
        bmesh.ops.create_uvsphere(bm, u_segments=20, v_segments=10, radius=r2,
                                  matrix=Matrix.Translation((0, 0, -length)))
        for f in bm.faces:
            f.smooth = True
        me = bpy.data.meshes.new(f"{self.aid}_{name}")
        bm.to_mesh(me)
        bm.free()
        me.materials.append(mat)
        ob = bpy.data.objects.new(f"{self.aid}_{name}", me)
        direction = math.atan2(b[1] - a[1], b[0] - a[0])
        ob.location = (a[0], y, a[1])
        ob.rotation_euler = (0.0, -(direction + HALF_PI), 0.0)
        self.link(ob)
        ob.parent = joint.obj
        return ob

    def limb(self, mat, joint, length, r1, r2, y=0.0, name="limb"):
        return self.segment(mat, joint, (0.0, 0.0), (0.0, -length), r1, r2, y=y, name=name)

    def sphere(self, mat, joint, at, r, y=0.0, name="ball", scale=(1.0, 1.0, 1.0)):
        bm = bmesh.new()
        bmesh.ops.create_uvsphere(bm, u_segments=20, v_segments=10, radius=r)
        for f in bm.faces:
            f.smooth = True
        me = bpy.data.meshes.new(f"{self.aid}_{name}")
        bm.to_mesh(me)
        bm.free()
        me.materials.append(mat)
        ob = bpy.data.objects.new(f"{self.aid}_{name}", me)
        ob.location = (at[0], y, at[1])
        ob.scale = scale
        self.link(ob)
        ob.parent = joint.obj
        return ob

    def spiral(self, mat, joint, center, r0, r1, turns, thickness, y, name="spiral"):
        """A flat Archimedean spiral tube in the XZ plane (the snail's shell line)."""
        cu = bpy.data.curves.new(f"{self.aid}_{name}", "CURVE")
        cu.dimensions = "3D"
        cu.bevel_depth = thickness
        cu.bevel_resolution = 3
        cu.use_fill_caps = True
        sp = cu.splines.new("POLY")
        n = 64
        sp.points.add(n - 1)
        for i in range(n):
            t = i / (n - 1)
            ang = HALF_PI + turns * TAU * t
            r = r1 + (r0 - r1) * t
            sp.points[i].co = (center[0] + r * math.cos(ang), y, center[1] + r * math.sin(ang), 1.0)
        cu.materials.append(mat)
        ob = bpy.data.objects.new(f"{self.aid}_{name}", cu)
        self.link(ob)
        ob.parent = joint.obj
        return ob


# ---------------------------------------------------------------- animals
#
# Each builder creates the rig and returns pose(p), which sets every joint for
# loop phase p in [0, 1). `n = p * cycles` runs that many gait cycles per loop.

def build_snail(rig):
    skin = rig.mat("skin", "#b0ad93")
    shell = rig.mat("shell", "#cf8a42", 0.35)
    line = rig.mat("line", "#6b3d1c")
    dark = rig.mat("dark", "#3d342d")

    root = Joint(rig, "root", None, (0.0, 0.0))
    # The foot: a chain of joints along the sole carries a forward-travelling
    # ripple (the snail's pedal wave) and the stretch/contract of the glide.
    foot = [Joint(rig, f"foot{i}", root, (-0.62 + 0.25 * i, -0.30)) for i in range(6)]
    for i, j in enumerate(foot):
        taper = 0.15 if i == 0 else 0.17
        rig.blob("Skin", skin, j, (0.0, 0.0), taper, size=(1.35, 1.0, 0.75))
    neck = Joint(rig, "neck", foot[5], (0.0, 0.0), rest=2.62)
    rig.blob("Skin", skin, neck, (0.0, -0.13), 0.16, size=(1.0, 1.0, 1.2))
    head = Joint(rig, "head", neck, (0.0, -0.27))
    rig.blob("Skin", skin, head, (0.0, 0.0), 0.16)
    stalks = []
    for k, (y, m) in enumerate(((-0.06, skin), (0.06, rig.mat("skin_far", "#8c8a74")))):
        st = Joint(rig, f"stalk{k}", head, (0.02, 0.02), y=y)
        rig.limb(m, st, 0.36, 0.034, 0.026, name=f"stalk{k}")
        rig.sphere(dark, st, (0.0, -0.36), 0.055, name=f"eye{k}")
        stalks.append(st)
    feeler = Joint(rig, "feeler", head, (0.07, -0.05), y=-0.06)
    rig.limb(skin, feeler, 0.12, 0.025, 0.018, name="feeler")

    body = Joint(rig, "shell", foot[2], (0.08, 0.30))
    rig.blob("Shell", shell, body, (0.0, 0.0), 0.43, size=(1.0, 0.75, 1.0))
    rig.blob("Shell", shell, body, (-0.08, -0.08), 0.33, size=(1.0, 0.8, 1.0))
    rig.spiral(line, body, (0.02, 0.03), 0.06, 0.34, 1.6, 0.022, y=-0.33)

    def pose(p):
        # One slow glide per loop: the front pushes ahead, the tail catches up.
        stretch = wave(p, 1.0, 0.25)
        for i, j in enumerate(foot):
            t = i / 5.0
            ripple = 0.018 * math.sin(TAU * (p * 2 - t * 1.5))  # wave runs forward
            j.d = (0.1 * stretch * (t - 0.4), ripple)
        root.d = (0.0, 0.0)
        body.d = (-0.02 * stretch, 0.02 * wave(p, 1.0, 0.4))
        body.a = 0.07 * wave(p, 1.0, 0.45)
        neck.a = neck.rest + 0.09 * wave(p, 1.0, 0.3)
        head.a = -0.6 - 0.06 * wave(p, 1.0, 0.35)
        # Eye stalks sway forward/back with lag and wiggle out of phase.
        stalks[0].set_world_angle(2.95 + 0.28 * wave(p, 1.0, 0.45) + 0.08 * wave(p * 2, 1.0, 0.1))
        stalks[1].set_world_angle(2.45 + 0.28 * wave(p, 1.0, 0.55) + 0.08 * wave(p * 2, 1.0, 0.6))
        feeler.set_world_angle(1.0 + 0.15 * wave(p, 1.0, 0.6))

    return pose


def build_turtle(rig):
    shell = rig.mat("shell", "#7ba64a", 0.4)
    rim = rig.mat("rim", "#557233", 0.5)
    skin = rig.mat("skin", "#c6c88a")
    far = rig.mat("far", "#92965e")
    dark = rig.mat("dark", "#22251c")
    ground = -0.64

    root = Joint(rig, "root", None, (0.0, 0.0))
    rig.blob("Shell", shell, root, (0.0, 0.08), 0.62, size=(1.0, 0.85, 0.6))
    rig.blob("Shell", shell, root, (0.05, 0.22), 0.4, size=(1.0, 0.8, 0.7))
    rig.blob("Rim", rim, root, (0.0, -0.13), 0.66, size=(1.0, 0.86, 0.22))
    rig.blob("Skin", skin, root, (0.0, -0.18), 0.5, size=(1.0, 0.7, 0.35))
    neck = Joint(rig, "neck", root, (0.5, -0.1), rest=1.83)
    rig.blob("Skin", skin, neck, (0.0, -0.12), 0.12, size=(1.6, 1.0, 1.0), angle=HALF_PI)
    head = Joint(rig, "head", neck, (0.0, -0.28))
    rig.blob("Skin", skin, head, (0.07, -0.04), 0.15, size=(1.25, 1.0, 1.0), angle=0.2)
    rig.sphere(dark, head, (0.12, -0.0), 0.032, y=-0.12, name="eye")
    tail = Joint(rig, "tail", root, (-0.62, -0.12))
    rig.segment(skin, tail, (0.0, 0.0), (-0.14, -0.03), 0.05, 0.02, name="tail")

    legs = []
    for name, hip, y, m, bend in (
            ("fore_far", (0.36, -0.2), 0.16, far, 1), ("hind_far", (-0.36, -0.2), 0.16, far, -1),
            ("fore_near", (0.36, -0.2), -0.18, skin, 1), ("hind_near", (-0.36, -0.2), -0.18, skin, -1)):
        leg = Leg(rig, name, root, hip, y, 0.23, 0.23, bend)
        rig.limb(m, leg.upper, 0.23, 0.1, 0.085, name=name + "_u")
        rig.limb(m, leg.lower, 0.23, 0.085, 0.08, name=name + "_l")
        rig.sphere(m, leg.end, (0.0, -0.03), 0.085, name=name + "_paw", scale=(0.7, 1.0, 1.4))
        legs.append((leg, name))
    # Trot: diagonal pairs move together, half a cycle apart.
    touchdown = {"fore_near": 0.0, "hind_far": 0.0, "fore_far": 0.5, "hind_near": 0.5}
    cx = {"fore": 0.42, "hind": -0.36}
    stance, reach = 0.55, 0.17

    def pose(p):
        n = p * rig.cycles
        # Weight shifts onto each diagonal pair: the shell dips at each
        # mid-stance and rocks fore/aft as the lead pair changes.
        root.d = (0.012 * math.sin(TAU * n), -0.018 * math.cos(2 * TAU * n) - 0.01)
        root.a = 0.05 * math.sin(TAU * n)
        neck.a = neck.rest - root.a + 0.07 * wave(n * 2, 1.0, 0.3)
        neck.d = (0.03 * wave(n * 2, 1.0, 0.15), 0.0)
        head.set_world_angle(-0.05 * math.sin(TAU * (n * 2 - 0.2)))
        tail.a = -0.15 * math.sin(TAU * n)
        for leg, name in legs:
            q = wrap(n - touchdown[name])
            x, z, _ = foot_path(q, stance, reach, 0.16, ground, cx[name.split("_")[0]])
            leg.reach((x, z + 0.06))
            leg.end_direction(0.0)

    return pose


def build_rabbit(rig):
    fur = rig.mat("fur", "#c9a27a")
    far = rig.mat("far", "#94765a")
    belly = rig.mat("tail", "#f1ebe2")
    inner = rig.mat("inner", "#d9a99a")
    dark = rig.mat("dark", "#241c17")
    ground = -0.55

    root = Joint(rig, "root", None, (0.0, 0.0))
    rear = Joint(rig, "rear", root, (-0.05, 0.0))
    front = Joint(rig, "front", root, (0.05, 0.0))
    rig.blob("Fur", fur, root, (0.0, 0.0), 0.2, size=(1.4, 1.0, 1.0))
    rig.blob("Fur", fur, rear, (-0.2, 0.04), 0.3, size=(1.1, 1.0, 1.0))
    rig.blob("Fur", fur, front, (0.2, -0.02), 0.2, size=(1.2, 1.0, 1.0))
    neck = Joint(rig, "neck", front, (0.32, 0.08))
    rig.blob("Fur", fur, neck, (0.0, 0.0), 0.13)
    head = Joint(rig, "head", neck, (0.08, 0.12))
    rig.blob("Fur", fur, head, (0.05, 0.0), 0.15, size=(1.25, 1.0, 1.0), angle=-0.3)
    rig.blob("Fur", fur, head, (0.2, -0.06), 0.09)
    rig.sphere(dark, head, (0.12, 0.03), 0.032, y=-0.12, name="eye")
    rig.sphere(dark, head, (0.27, -0.06), 0.022, y=-0.06, name="nose")
    ears = []
    for k, (y, m) in enumerate(((0.05, far), (-0.05, fur))):
        ear = Joint(rig, f"ear{k}", head, (-0.02, 0.1), y=y)
        rig.segment(m, ear, (0.0, 0.0), (0.0, -0.42), 0.05, 0.045, name=f"ear{k}")
        if k == 1:
            rig.segment(inner, ear, (0.0, -0.06), (0.0, -0.36), 0.025, 0.02, y=-0.03, name="ear_in")
        ears.append(ear)
    rig.sphere(belly, rear, (-0.47, 0.12), 0.09, y=-0.02, name="tail")

    FOOT = 0.3
    hinds, fores = [], []
    for name, y, m in (("hind_far", 0.12, far), ("hind_near", -0.14, fur)):
        leg = Leg(rig, name, rear, (-0.2, -0.12), y, 0.24, 0.24, 1)
        rig.blob("Thigh" if m is fur else "ThighFar", m, leg.upper, (0.02, -0.08), 0.15,
                 size=(1.0, 0.8, 1.3))
        rig.limb(m, leg.upper, 0.24, 0.09, 0.06, name=name + "_u")
        rig.limb(m, leg.lower, 0.24, 0.06, 0.045, name=name + "_l")
        rig.limb(m, leg.end, FOOT, 0.05, 0.045, name=name + "_foot")
        hinds.append(leg)
    for name, y, m in (("fore_far", 0.08, far), ("fore_near", -0.1, fur)):
        leg = Leg(rig, name, front, (0.22, -0.1), y, 0.22, 0.22, 1)
        rig.limb(m, leg.upper, 0.22, 0.065, 0.048, name=name + "_u")
        rig.limb(m, leg.lower, 0.22, 0.048, 0.042, name=name + "_l")
        rig.sphere(m, leg.end, (0.0, -0.02), 0.045, name=name + "_paw", scale=(1.0, 1.0, 1.4))
        fores.append(leg)

    # Hop phases (n): 0 push-off, 0.15-0.4 extended flight, 0.42 forepaws land,
    # 0.62 hind feet land ahead of them, 0.8 crouch, then push again.
    z = loop_curve([(0.0, 0.03), (0.15, 0.17), (0.3, 0.22), (0.45, 0.07), (0.55, -0.01),
                    (0.7, -0.05), (0.85, -0.07), (0.95, -0.03)])
    pitch = loop_curve([(0.0, 0.2), (0.18, 0.12), (0.33, -0.02), (0.47, -0.24), (0.6, -0.16),
                        (0.75, 0.02), (0.9, 0.12)])
    flex = loop_curve([(0.0, -0.15), (0.22, -0.38), (0.42, -0.15), (0.62, 0.2), (0.8, 0.42),
                       (0.93, 0.15)])
    ear = loop_curve([(0.0, -0.12), (0.2, -0.38), (0.38, -0.2), (0.52, 0.18), (0.66, 0.12),
                      (0.84, -0.02)])
    v = 0.8  # treadmill speed, units per hop
    hind_stance, fore_stance = 0.46, 0.22
    # Hind toe in swing (u = 0 lift-off .. 1 touchdown): it trails far behind
    # through the stretched flight, then sweeps forward to land ahead of the
    # forepaws. psi is the heel->toe direction: it peels up through push-off,
    # trails in flight and flattens for landing.
    hind_x = path_curve([(0.0, 0.2 - v * hind_stance), (0.15, -0.5), (0.3, -0.75), (0.5, -0.55),
                         (0.72, -0.1), (1.0, 0.2)])
    hind_z = path_curve([(0.0, 0.0), (0.15, 0.03), (0.3, 0.06), (0.5, 0.18), (0.72, 0.16),
                         (1.0, 0.0)])
    hind_psi = path_curve([(0.0, -1.2), (0.15, -1.9), (0.3, -2.35), (0.5, -2.1), (0.72, -1.0),
                           (1.0, -0.15)])
    hind_td, fore_td = 0.62, 0.42

    def pose(p):
        n = wrap(p * rig.cycles + 0.85)  # frame 0 is the crouch, the calmest pose
        root.d = (0.0, z(n))
        root.a = pitch(n)
        f = flex(n)
        rear.a, rear.d = 0.55 * f, (0.07 * f, 0.0)
        front.a, front.d = -0.35 * f, (-0.04 * f, 0.0)
        neck.set_world_angle(0.0)
        head.set_world_angle(0.35 * pitch(n - 0.05))
        for k, e in enumerate(ears):
            e.set_world_angle(3.65 - 0.3 * k - ear(n - 0.03 * k))
        for k, leg in enumerate(hinds):
            q = wrap(n - hind_td - 0.02 * k)
            if q < hind_stance:  # toe planted, sliding back with the ground
                tx, tz = 0.2 - v * q, ground
                psi = -0.15 * max(0.0, 1.0 - q / 0.04)
                psi -= 1.2 * max(0.0, (q - 0.34) / (hind_stance - 0.34))
            else:
                u = (q - hind_stance) / (1.0 - hind_stance)
                tx, tz, psi = hind_x(u), ground + hind_z(u), hind_psi(u)
            heel = (tx - FOOT * math.cos(psi), tz - FOOT * math.sin(psi) + 0.045)
            leg.reach(heel)
            leg.end_direction(psi)
        for k, leg in enumerate(fores):
            q = wrap(n - fore_td - 0.04 * (1 - k))
            reach = v * fore_stance / 2
            tx, tz, _ = foot_path(q, fore_stance, reach, 0.15, ground, 0.36, peak=0.8)
            leg.reach((tx, tz + 0.045))
            leg.end_direction(0.0)

    return pose


def build_horse(rig):
    coat = rig.mat("coat", "#b06e40")
    far = rig.mat("far", "#7a4b2c")
    hair = rig.mat("hair", "#2f211a")
    hoof = rig.mat("hoof", "#26201c")
    ground = -0.92

    root = Joint(rig, "root", None, (0.0, 0.0))
    rig.blob("Coat", coat, root, (0.0, 0.03), 0.25, size=(2.0, 1.0, 1.0))
    rig.blob("Coat", coat, root, (0.36, 0.02), 0.25)
    rig.blob("Coat", coat, root, (-0.36, 0.07), 0.27)
    rig.blob("Coat", coat, root, (0.3, 0.17), 0.16)
    neck = Joint(rig, "neck", root, (0.42, 0.16), rest=2.4)
    rig.blob("Coat", coat, neck, (0.0, -0.16), 0.17, size=(1.4, 1.0, 1.0), angle=HALF_PI)
    rig.blob("Coat", coat, neck, (0.0, -0.38), 0.125, size=(1.3, 1.0, 1.0), angle=HALF_PI)
    rig.segment(hair, neck, (0.13, -0.02), (0.09, -0.5), 0.055, 0.04, name="mane")
    head = Joint(rig, "head", neck, (0.0, -0.52))
    rig.blob("Coat", coat, head, (0.0, -0.08), 0.135)
    rig.blob("Coat", coat, head, (0.0, -0.31), 0.095, size=(1.5, 1.0, 1.0), angle=HALF_PI)
    rig.segment(coat, head, (0.06, 0.0), (0.2, 0.02), 0.035, 0.015, y=-0.04, name="ear")
    rig.sphere(hoof, head, (0.06, -0.12), 0.028, y=-0.1, name="eye")
    tail = [Joint(rig, "tail0", root, (-0.6, 0.2))]
    for i in range(1, 4):
        tail.append(Joint(rig, f"tail{i}", tail[-1], (0.0, -0.14)))
    for i, t in enumerate(tail):
        rig.limb(hair, t, 0.14, 0.065 - 0.008 * i, 0.06 - 0.008 * i, name=f"tail{i}")

    legs = {}
    for name, hip, y, m, bend, l1 in (
            ("hind_far", (-0.4, -0.04), 0.14, far, -1, 0.46),
            ("fore_far", (0.36, -0.1), 0.14, far, 1, 0.42),
            ("hind_near", (-0.4, -0.04), -0.14, coat, -1, 0.46),
            ("fore_near", (0.36, -0.1), -0.14, coat, 1, 0.42)):
        leg = Leg(rig, name, root, hip, y, l1, 0.42, bend)
        top = 0.1 if name.startswith("hind") else 0.08
        rig.limb(m, leg.upper, l1, top, 0.05, name=name + "_u")
        rig.limb(m, leg.lower, 0.42, 0.045, 0.04, name=name + "_l")
        rig.segment(hoof, leg.end, (0.0, 0.03), (0.0, -0.03), 0.05, 0.055, name=name + "_hoof")
        legs[name] = leg
    # Transverse gallop: four beats (hind far, hind near, fore far, fore near)
    # then one suspension with the legs gathered under the body.
    touchdown = {"hind_far": 0.0, "hind_near": 0.12, "fore_far": 0.34, "fore_near": 0.46}
    cx = {"fore": 0.5, "hind": -0.36}
    reach = {"fore": 0.36, "hind": 0.34}
    lift = {"fore": 0.42, "hind": 0.3}
    stance = 0.3

    def pose(p):
        n = wrap(p * rig.cycles)
        pitch = wave(n, 0.085, 0.2)          # forehand rises during the hind push
        root.d = (0.0, wave(n, 0.045, 0.88))  # highest in the suspension
        root.a = pitch
        neck.a = neck.rest - 1.6 * pitch + wave(n, 0.06, 0.7)  # counter-motion
        head.a = -1.6 + 0.5 * pitch
        for i, t in enumerate(tail):
            base = -1.25 if i == 0 else 0.12
            t.a = base + wave(n - 0.12 * i, 0.12 + 0.05 * i, 0.3) - (pitch if i == 0 else 0.0)
        for name, leg in legs.items():
            kind = name.split("_")[0]
            q = wrap(n - touchdown[name])
            x, zz, u = foot_path(q, stance, reach[kind], lift[kind], ground, cx[kind], peak=0.6)
            phi = leg.reach((x, zz + 0.06))
            if u < 0:
                leg.end_direction(-HALF_PI)
            else:
                leg.end_direction(phi - 0.6 * math.sin(math.pi * u))

    return pose


def build_cheetah(rig):
    fur = rig.mat("fur", "#e2ad4f")
    far = rig.mat("far", "#a97c35")
    spot = rig.mat("spot", "#3a2a18")
    pale = rig.mat("pale", "#efdcb0")
    ground = -0.86

    root = Joint(rig, "root", None, (0.0, 0.0))
    chest = Joint(rig, "chest", root, (0.05, 0.0))
    hip = Joint(rig, "hip", root, (-0.05, 0.0))
    rig.blob("Fur", fur, root, (0.0, 0.0), 0.15, size=(1.6, 1.0, 1.0))
    rig.blob("Fur", fur, chest, (0.3, -0.02), 0.2, size=(1.2, 1.0, 1.15))
    rig.blob("Fur", fur, chest, (0.12, 0.0), 0.16)
    rig.blob("Fur", fur, hip, (-0.25, 0.03), 0.15)
    rig.blob("Fur", fur, hip, (-0.44, 0.05), 0.16)
    neck = Joint(rig, "neck", chest, (0.46, 0.07))
    rig.blob("Fur", fur, neck, (0.06, 0.04), 0.11, size=(1.4, 1.0, 1.0), angle=0.5)
    head = Joint(rig, "head", neck, (0.18, 0.12))
    rig.blob("Fur", fur, head, (0.02, 0.0), 0.12, size=(1.15, 1.0, 1.0))
    rig.blob("Fur", fur, head, (0.14, -0.04), 0.07)
    rig.segment(fur, head, (-0.05, 0.08), (-0.07, 0.15), 0.035, 0.025, y=-0.03, name="ear")
    rig.sphere(spot, head, (0.07, 0.03), 0.026, y=-0.1, name="eye")
    rig.segment(spot, head, (0.07, 0.0), (0.12, -0.09), 0.012, 0.012, y=-0.1, name="tear")
    for k, (j, at) in enumerate(((chest, (0.28, 0.1)), (chest, (0.12, 0.05)), (root, (-0.02, 0.07)),
                                 (hip, (-0.24, 0.1)), (hip, (-0.42, 0.02)), (chest, (0.2, -0.06)),
                                 (hip, (-0.32, -0.04)))):
        rig.sphere(spot, j, at, 0.026, y=-0.25, name=f"spot{k}", scale=(1.0, 0.4, 1.0))
    tail = [Joint(rig, "tail0", hip, (-0.6, 0.08))]
    for i in range(1, 6):
        tail.append(Joint(rig, f"tail{i}", tail[-1], (0.0, -0.12)))
    for i, t in enumerate(tail):
        m = spot if i == 5 else fur
        rig.limb(m, t, 0.12, 0.04 - 0.003 * i, 0.037 - 0.003 * i, name=f"tail{i}")

    legs = {}
    for name, parent, hip_at, y, m, bend in (
            ("hind_far", hip, (-0.42, -0.06), 0.1, far, -1),
            ("fore_far", chest, (0.3, -0.12), 0.1, far, 1),
            ("hind_near", hip, (-0.42, -0.06), -0.1, fur, -1),
            ("fore_near", chest, (0.3, -0.12), -0.1, fur, 1)):
        l1 = 0.42 if name.startswith("hind") else 0.38
        leg = Leg(rig, name, parent, hip_at, y, l1, 0.38, bend)
        top = 0.09 if name.startswith("hind") else 0.072
        rig.limb(m, leg.upper, l1, top, 0.045, name=name + "_u")
        rig.limb(m, leg.lower, 0.38, 0.042, 0.034, name=name + "_l")
        rig.sphere(m, leg.end, (0.0, -0.02), 0.04, name=name + "_paw", scale=(0.9, 1.0, 1.5))
        legs[name] = leg
    # Rotary gallop with two suspensions: the extended one after the hind
    # push-off, the gathered one (spine flexed, feet crossed) after the fore.
    touchdown = {"fore_far": 0.0, "fore_near": 0.08, "hind_far": 0.5, "hind_near": 0.58}
    cx = {"fore": 0.36, "hind": -0.28}
    reach = {"fore": 0.32, "hind": 0.36}
    stance = 0.28

    def pose(p):
        n = wrap(p * rig.cycles)
        flex = wave(n, 1.0, 0.43)  # +1 gathered (arched), -1 extended
        root.d = (0.0, wave(2 * n, 0.03, 0.86))
        root.a = wave(n, 0.07, 0.7)
        chest.a, chest.d = -0.2 * flex, (-0.05 * (flex + 1), 0.0)
        hip.a, hip.d = 0.3 * flex, (0.09 * (flex + 1), 0.0)
        neck.set_world_angle(-0.05 + 0.06 * flex)
        head.set_world_angle(0.0)  # head stays level: the cheetah's gaze lock
        # Tail counterbalances the hips with a travelling lag toward the tip.
        tail[0].set_world_angle(-HALF_PI * 1.0 - 0.15 - 0.6 * hip.world()[1])
        for i, t in enumerate(tail[1:], 1):
            t.a = -0.12 + wave(n - 0.09 * i, 0.14, 0.95)
        for name, leg in legs.items():
            kind = name.split("_")[0]
            q = wrap(n - touchdown[name])
            x, zz, u = foot_path(q, stance, reach[kind], 0.3, ground, cx[kind], peak=0.6)
            phi = leg.reach((x, zz + 0.04))
            leg.end_direction(0.0 if u < 0 else phi - 0.8 * math.sin(math.pi * u))

    return pose


BUILDERS = {
    # id: (builder, gait cycles per 24-frame loop)
    "snail": (build_snail, 1),
    "turtle": (build_turtle, 2),
    "rabbit": (build_rabbit, 2),
    "horse": (build_horse, 3),
    "cheetah": (build_cheetah, 4),
}


# ---------------------------------------------------------------- scene

def make_camera():
    cam = bpy.data.cameras.new(C.CAMERA_NAME)
    cam.type = "ORTHO"
    cam.ortho_scale = C.ORTHO_SCALE
    cam.clip_start, cam.clip_end = 0.1, 2.0 * C.CAMERA_DISTANCE
    ob = bpy.data.objects.new(C.CAMERA_NAME, cam)
    ob.location = (0.0, -C.CAMERA_DISTANCE, 0.0)
    ob.rotation_euler = (HALF_PI, 0.0, 0.0)
    return ob


def bounds(scene):
    """Union XZ bounds over all loop frames, and the range of per-frame bbox centres."""
    lo, hi = [math.inf, math.inf], [-math.inf, -math.inf]
    clo, chi = [math.inf, math.inf], [-math.inf, -math.inf]
    for f in range(C.FRAME_COUNT):
        flo, fhi = [math.inf, math.inf], [-math.inf, -math.inf]
        scene.frame_set(f)
        with bpy.context.temp_override(scene=scene, view_layer=scene.view_layers[0]):
            dg = bpy.context.evaluated_depsgraph_get()
        for ob in scene.objects:
            if ob.type not in {"MESH", "META", "CURVE"}:
                continue
            ev = ob.evaluated_get(dg)
            me = ev.to_mesh()
            if me is None:
                continue
            mw = ev.matrix_world
            for v in me.vertices:
                w = mw @ v.co
                flo[0], fhi[0] = min(flo[0], w.x), max(fhi[0], w.x)
                flo[1], fhi[1] = min(flo[1], w.z), max(fhi[1], w.z)
            ev.to_mesh_clear()
        for k in (0, 1):
            lo[k], hi[k] = min(lo[k], flo[k]), max(hi[k], fhi[k])
            c = (flo[k] + fhi[k]) / 2
            clo[k], chi[k] = min(clo[k], c), max(chi[k], c)
    return lo, hi, clo, chi


def build(aid, camera):
    builder, cycles = BUILDERS[aid]
    scene = bpy.data.scenes.new(aid)
    scene.frame_start, scene.frame_end = 0, C.FRAME_COUNT - 1
    scene.render.fps = C.FPS
    scene.collection.objects.link(camera)
    scene.camera = camera
    rig = Rig(scene, aid, cycles)
    pose = builder(rig)
    for f in range(C.FRAME_COUNT + 1):
        pose((f % C.FRAME_COUNT) / C.FRAME_COUNT)
        for j in rig.joints:
            j.key(f)
    # Normalise: centre the range of per-frame bbox centres on the camera (so
    # no frame drifts far off centre), then scale so the all-frame bounds stay
    # within FIT_PX around that centre. Every animal fills its cell alike.
    lo, hi, clo, chi = bounds(scene)
    mid = [(clo[k] + chi[k]) / 2 for k in (0, 1)]
    half = max(max(mid[k] - lo[k], hi[k] - mid[k]) for k in (0, 1))
    s = (C.FIT_PX / C.PX_PER_UNIT) / (2 * half)
    rig.fit.scale = (s, s, s)
    rig.fit.location = (-s * mid[0], 0.0, -s * mid[1])
    scene.frame_set(0)
    spread = max(chi[k] - clo[k] for k in (0, 1)) * s * C.PX_PER_UNIT / 2
    print(f"[build] {aid}: cycles={cycles} joints={len(rig.joints)} scale={s:.3f} "
          f"max centre drift={spread:.1f}px")
    return scene


def main():
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    if len(argv) != 2 or argv[0] != "--output":
        raise SystemExit("usage: ... --python build_scene.py -- --output burn_animals.blend")
    out = os.path.abspath(argv[1])
    default = bpy.data.scenes[0]
    camera = make_camera()
    scenes = [build(aid, camera) for aid in C.ANIMALS]
    for ob in list(default.objects):
        bpy.data.objects.remove(ob)
    bpy.data.scenes.remove(default)
    for scene in scenes:
        scene.render.engine = "BLENDER_WORKBENCH"
    bpy.context.preferences.filepaths.save_version = 0  # no burn_animals.blend1 backup
    bpy.ops.wm.save_as_mainfile(filepath=out, compress=True)
    print(f"[build] wrote {out}")


if __name__ == "__main__":
    main()
