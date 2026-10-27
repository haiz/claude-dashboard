"""Renders burn_animals.blend into one sprite sheet per animal plus manifest.json.

Run (from this directory):
    <blender> --background --factory-startup burn_animals.blend \
        --python-exit-code 1 --python render.py -- --out .

For every scene (named by asset id) it renders frames 0..23 with Workbench at
CELL * SUPERSAMPLE px, adds a dark outline and a faint light rim at that resolution, box-filters down
to CELL px in premultiplied alpha, and packs the cells row-major (6 x 4) into
<id>.png. PNGs are written by a small encoder here (fixed zlib level, filter 0,
no metadata chunks) so identical pixels always give identical bytes.

Every setting that affects pixels is set here, not trusted from the .blend:
engine, AA samples, lighting, color management, resolution, film, camera,
frame range. The script refuses to run on any Blender other than the one
named in BLENDER_VERSION.
"""

import hashlib
import json
import os
import shutil
import struct
import sys
import tempfile
import zlib

import bpy
import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
sys.dont_write_bytecode = True  # keep __pycache__ out of the source tree
sys.path.insert(0, HERE)
import common as C  # noqa: E402

# Two rings around the silhouette, in hi-res px (divide by SUPERSAMPLE for
# cell px): a dark outline that reads on light themes and, outside it, a faint
# light rim that reads on dark themes and vanishes on light ones.
OUTLINE_RGB, OUTLINE_ALPHA, OUTLINE_RADIUS = (0x24 / 255, 0x1D / 255, 0x18 / 255), 0.92, 6
RIM_RGB, RIM_ALPHA, RIM_RADIUS = (1.0, 1.0, 1.0), 0.3, 10


def fail(msg):
    print(f"[render] ERROR: {msg}", file=sys.stderr)
    sys.stderr.flush()
    sys.exit(1)


def check_version():
    with open(os.path.join(HERE, "BLENDER_VERSION"), encoding="utf-8") as fh:
        want = fh.readline().strip()
    have = f"Blender {bpy.app.version_string}"
    if have != want:
        fail(f"Blender version mismatch: running '{have}', BLENDER_VERSION pins '{want}'")
    return want


def configure(scene):
    hi = C.CELL * C.SUPERSAMPLE
    r = scene.render
    r.engine = "BLENDER_WORKBENCH"
    r.resolution_x = r.resolution_y = hi
    r.resolution_percentage = 100
    r.pixel_aspect_x = r.pixel_aspect_y = 1.0
    r.film_transparent = True
    r.use_border = False
    r.use_stamp = False
    r.use_compositing = False
    r.use_sequencer = False
    r.threads_mode = "FIXED"
    r.threads = 1
    r.fps, r.fps_base = C.FPS, 1.0
    s = r.image_settings
    s.file_format = "PNG"
    s.color_mode = "RGBA"
    s.color_depth = "16"
    s.compression = 0
    scene.frame_start, scene.frame_end = 0, C.FRAME_COUNT - 1

    v = scene.view_settings
    scene.display_settings.display_device = "sRGB"
    v.view_transform = "Standard"
    v.look = "None"
    v.exposure = 0.0
    v.gamma = 1.0
    v.use_curve_mapping = False

    d = scene.display
    d.render_aa = "16"
    sh = d.shading
    sh.light = "STUDIO"
    sh.studio_light = "outdoor.sl"  # soft key + sky fill, bundled with Blender
    sh.studiolight_rotate_z = 0.0
    sh.use_world_space_lighting = False
    sh.color_type = "MATERIAL"
    sh.show_specular_highlight = True
    sh.show_cavity = False
    sh.show_shadows = False
    sh.show_object_outline = False
    sh.show_xray = False
    sh.show_backface_culling = False
    sh.use_dof = False

    cam = scene.camera
    if cam is None or cam.name != C.CAMERA_NAME:
        fail(f"scene '{scene.name}' has no {C.CAMERA_NAME}")
    cam.location = (0.0, -C.CAMERA_DISTANCE, 0.0)
    cam.rotation_euler = (1.5707963267948966, 0.0, 0.0)
    cam.data.type = "ORTHO"
    cam.data.ortho_scale = C.ORTHO_SCALE
    cam.data.shift_x = cam.data.shift_y = 0.0


def load_rgba(path):
    """Straight-alpha float RGBA, top row first, values exactly as stored."""
    img = bpy.data.images.load(path, check_existing=False)
    img.colorspace_settings.name = "Non-Color"
    img.alpha_mode = "STRAIGHT"
    w, h = img.size
    buf = np.empty(w * h * 4, dtype=np.float32)
    img.pixels.foreach_get(buf)
    bpy.data.images.remove(img)
    return buf.reshape(h, w, 4)[::-1].astype(np.float64)


def dilate(alpha, radius):
    """Max filter over a disc of `radius` px."""
    h, w = alpha.shape
    pad = np.pad(alpha, radius)
    out = np.zeros_like(alpha)
    for dy in range(-radius, radius + 1):
        for dx in range(-radius, radius + 1):
            if dx * dx + dy * dy <= radius * radius:
                np.maximum(out, pad[radius + dy:radius + dy + h, radius + dx:radius + dx + w], out=out)
    return out


def over(top_prem, top_a, prem, a):
    return top_prem + prem * (1.0 - top_a)[..., None], top_a + a * (1.0 - top_a)


def to_cell(rgba):
    """Outline at hi-res, then box-filter to CELL px. Returns uint8 straight RGBA."""
    # Workbench's AA leaves solid interiors at alpha 254/255; make them opaque.
    sa = np.where(rgba[..., 3] >= 0.99, 1.0, rgba[..., 3])
    rim_a = dilate(sa, RIM_RADIUS) * RIM_ALPHA
    prem, a = np.array(RIM_RGB)[None, None, :] * rim_a[..., None], rim_a
    line_a = dilate(sa, OUTLINE_RADIUS) * OUTLINE_ALPHA
    prem, a = over(np.array(OUTLINE_RGB)[None, None, :] * line_a[..., None], line_a, prem, a)
    prem, a = over(rgba[..., :3] * sa[..., None], sa, prem, a)
    k = C.SUPERSAMPLE
    n = C.CELL
    prem = prem.reshape(n, k, n, k, 3).mean(axis=(1, 3))
    a = a.reshape(n, k, n, k).mean(axis=(1, 3))
    a8 = np.rint(np.clip(a, 0.0, 1.0) * 255.0)
    safe = np.where(a > 0, a, 1.0)
    rgb8 = np.rint(np.clip(prem / safe[..., None], 0.0, 1.0) * 255.0)
    rgb8[a8 == 0] = 0
    return np.dstack([rgb8, a8]).astype(np.uint8)


def write_png(path, rgba):
    h, w, _ = rgba.shape
    raw = b"".join(b"\x00" + rgba[y].tobytes() for y in range(h))

    def chunk(kind, data):
        return (struct.pack(">I", len(data)) + kind + data
                + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF))

    png = (b"\x89PNG\r\n\x1a\n"
           + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
           + chunk(b"IDAT", zlib.compress(raw, 9))
           + chunk(b"IEND", b""))
    with open(path, "wb") as fh:
        fh.write(png)


def sha256(path):
    with open(path, "rb") as fh:
        return hashlib.sha256(fh.read()).hexdigest()


def render_animal(scene, tmp):
    configure(scene)
    sheet = np.zeros((C.ROWS * C.CELL, C.COLUMNS * C.CELL, 4), dtype=np.uint8)
    for f in range(C.FRAME_COUNT):
        path = os.path.join(tmp, f"{scene.name}_{f:02d}.png")
        scene.render.filepath = path
        # Background Blender only re-evaluates animation for the context
        # scene, so make this scene the context before changing frames.
        with bpy.context.temp_override(scene=scene, view_layer=scene.view_layers[0]):
            scene.frame_set(f)
            bpy.ops.render.render(write_still=True)
        cell = to_cell(load_rgba(path))
        row, col = divmod(f, C.COLUMNS)
        sheet[row * C.CELL:(row + 1) * C.CELL, col * C.CELL:(col + 1) * C.CELL] = cell
    return sheet


def main():
    version = check_version()
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    if len(argv) != 2 or argv[0] != "--out":
        fail("usage: ... --python render.py -- --out <dir>")
    out = os.path.abspath(argv[1])
    os.makedirs(out, exist_ok=True)
    blend = bpy.data.filepath
    if not blend:
        fail("open burn_animals.blend before --python render.py")
    missing = [a for a in C.ANIMALS if a not in bpy.data.scenes]
    if missing:
        fail(f"burn_animals.blend lacks scenes: {missing}")

    tmp = tempfile.mkdtemp(prefix="burn-animals-")
    try:
        for aid in C.ANIMALS:
            sheet = render_animal(bpy.data.scenes[aid], tmp)
            write_png(os.path.join(out, f"{aid}.png"), sheet)
            print(f"[render] {aid}.png")
    finally:
        shutil.rmtree(tmp, ignore_errors=True)

    manifest = {
        "schemaVersion": 1,
        "frameCount": C.FRAME_COUNT,
        "framesPerSecond": C.FPS,
        "columns": C.COLUMNS,
        "rows": C.ROWS,
        "cellWidth": C.CELL,
        "cellHeight": C.CELL,
        "sheetWidth": C.COLUMNS * C.CELL,
        "sheetHeight": C.ROWS * C.CELL,
        "frameOrder": "row-major from the top-left cell",
        "blenderVersion": version,
        "sourceRevision": "sha256:" + sha256(blend),
        "animals": [{"id": aid, "sheet": f"{aid}.png"} for aid in C.ANIMALS],
    }
    with open(os.path.join(out, "manifest.json"), "w", encoding="utf-8", newline="\n") as fh:
        json.dump(manifest, fh, indent=2)
        fh.write("\n")
    print(f"[render] manifest.json ({manifest['sourceRevision']})")


if __name__ == "__main__":
    main()
