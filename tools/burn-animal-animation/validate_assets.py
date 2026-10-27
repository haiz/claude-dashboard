"""Validates burn-animal sprite sheets and their manifest. Needs Pillow, not Blender.

    python3 -I validate_assets.py <dir> [<copy-dir> ...]

Checks <dir>/manifest.json and every sheet it lists. Each further directory
must hold byte-identical copies of the manifest and the sheets (the app
resource copies). Exits 1 and names the animal/frame for every violation.

The numbers below restate the asset contract on purpose (they are not
imported from common.py), so a wrong constant there cannot pass its own check.
"""

import json
import os
import sys

from PIL import Image

IDS = ("snail", "turtle", "rabbit", "horse", "cheetah")
FRAMES, FPS = 24, 12
COLUMNS, ROWS, CELL = 6, 4, 128
SHEET_W, SHEET_H = COLUMNS * CELL, ROWS * CELL
SUBJECT = 112           # subject box, centred in the cell
MAX_CENTER_OFFSET = 8   # px, bbox centre to cell centre
MAX_LOOP_AREA_DIFF = 0.10
MAX_LOOP_CENTER_SHIFT = 4
MAX_SHEET_BYTES = 2 * 1024 * 1024
PNG_MAGIC = b"\x89PNG\r\n\x1a\n"
MANIFEST_KEYS = {
    "schemaVersion": 1, "frameCount": FRAMES, "framesPerSecond": FPS,
    "columns": COLUMNS, "rows": ROWS, "cellWidth": CELL, "cellHeight": CELL,
    "sheetWidth": SHEET_W, "sheetHeight": SHEET_H,
}


def frame_stats(alpha):
    """bbox (x0, y0, x1, y1 exclusive), alpha-area, centroid of a cell's alpha."""
    w, h = alpha.size
    px = alpha.load()
    area = 0.0
    sx = sy = 0.0
    x0 = y0 = None
    x1 = y1 = -1
    for y in range(h):
        for x in range(w):
            a = px[x, y]
            if a:
                area += a / 255.0
                sx += x * a / 255.0
                sy += y * a / 255.0
                x0 = x if x0 is None or x < x0 else x0
                y0 = y if y0 is None or y < y0 else y0
                x1, y1 = max(x1, x), max(y1, y)
    if x0 is None:
        return None, 0.0, None
    return (x0, y0, x1 + 1, y1 + 1), area, ((sx / area) + 0.5, (sy / area) + 0.5)


def check_sheet(path, aid, errors):
    def err(msg):
        errors.append(f"{aid}: {msg}")

    size = os.path.getsize(path)
    if size > MAX_SHEET_BYTES:
        err(f"sheet is {size} bytes, over {MAX_SHEET_BYTES}")
    with open(path, "rb") as fh:
        if fh.read(8) != PNG_MAGIC:
            err("sheet is not a PNG")
            return
    im = Image.open(path)
    im.load()
    if im.mode != "RGBA":
        err(f"sheet mode is {im.mode}, want RGBA (an alpha channel)")
        return
    if im.size != (SHEET_W, SHEET_H):
        err(f"sheet is {im.size[0]}x{im.size[1]}, want {SHEET_W}x{SHEET_H}")
        return
    alpha = im.getchannel("A")
    margin = (CELL - SUBJECT) // 2
    c = CELL / 2.0
    stats = []
    for f in range(FRAMES):
        r, col = divmod(f, COLUMNS)
        cell = alpha.crop((col * CELL, r * CELL, (col + 1) * CELL, (r + 1) * CELL))
        where = f"frame {f} (row {r}, col {col})"
        bbox, area, centroid = frame_stats(cell)
        stats.append((bbox, area, centroid))
        if bbox is None:
            err(f"{where} is empty")
            continue
        x0, y0, x1, y1 = bbox
        if x0 < margin or y0 < margin or x1 > CELL - margin or y1 > CELL - margin:
            err(f"{where} has non-transparent pixels outside the centred {SUBJECT}x{SUBJECT} "
                f"subject box (bbox {bbox}); an opaque background or oversized subject")
        if x1 - x0 > SUBJECT or y1 - y0 > SUBJECT:
            err(f"{where} bbox {x1 - x0}x{y1 - y0} exceeds {SUBJECT}x{SUBJECT}")
        bx, by = (x0 + x1) / 2.0, (y0 + y1) / 2.0
        off = max(abs(bx - c), abs(by - c))
        if off > MAX_CENTER_OFFSET:
            err(f"{where} bbox centre ({bx:.1f}, {by:.1f}) is {off:.1f} px from the cell "
                f"centre, over {MAX_CENTER_OFFSET}")
    good = [s for s in stats if s[0] is not None]
    if good:
        wmax = max(b[2] - b[0] for b, _, _ in good)
        hmax = max(b[3] - b[1] for b, _, _ in good)
        omax = max(max(abs((b[0] + b[2]) / 2 - c), abs((b[1] + b[3]) / 2 - c)) for b, _, _ in good)
        print(f"  {aid}: {size} bytes, max bbox {wmax}x{hmax}, max centre offset {omax:.1f} px")
    first, last = stats[0], stats[-1]
    if first[0] is not None and last[0] is not None:
        diff = abs(first[1] - last[1]) / max(first[1], last[1])
        print(f"  {aid}: loop frame 0 vs {FRAMES - 1}: area diff {diff:.1%}, centroid shift "
              f"{((first[2][0] - last[2][0]) ** 2 + (first[2][1] - last[2][1]) ** 2) ** 0.5:.2f} px")
        if diff >= MAX_LOOP_AREA_DIFF:
            err(f"frame 0 vs frame {FRAMES - 1}: alpha-area differs by {diff:.1%}, "
                f"must be under {MAX_LOOP_AREA_DIFF:.0%}")
        for label, a, b in (("centroid", first[2], last[2]),
                            ("bbox centre", ((first[0][0] + first[0][2]) / 2, (first[0][1] + first[0][3]) / 2),
                             ((last[0][0] + last[0][2]) / 2, (last[0][1] + last[0][3]) / 2))):
            shift = ((a[0] - b[0]) ** 2 + (a[1] - b[1]) ** 2) ** 0.5
            if shift >= MAX_LOOP_CENTER_SHIFT:
                err(f"frame 0 vs frame {FRAMES - 1}: subject {label} shifts {shift:.1f} px, "
                    f"must be under {MAX_LOOP_CENTER_SHIFT}")
    return stats


def check_manifest(root, errors):
    path = os.path.join(root, "manifest.json")
    try:
        with open(path, encoding="utf-8") as fh:
            m = json.load(fh)
    except (OSError, ValueError) as e:
        errors.append(f"manifest: cannot read {path}: {e}")
        return None
    for key, want in MANIFEST_KEYS.items():
        if m.get(key) != want:
            errors.append(f"manifest: {key} is {m.get(key)!r}, want {want!r}")
    if not str(m.get("blenderVersion", "")).startswith("Blender "):
        errors.append(f"manifest: blenderVersion {m.get('blenderVersion')!r} is not a Blender version")
    rev = str(m.get("sourceRevision", ""))
    if not (rev.startswith("sha256:") and len(rev) == 7 + 64):
        errors.append(f"manifest: sourceRevision {rev!r} is not sha256:<64 hex>")
    animals = m.get("animals")
    if not isinstance(animals, list):
        errors.append("manifest: animals is not a list")
        return None
    ids = [a.get("id") for a in animals]
    if tuple(ids) != IDS:
        errors.append(f"manifest: animal ids {ids} != {list(IDS)} (stable ids, ladder order)")
    for a in animals:
        want = f"{a.get('id')}.png"
        if a.get("sheet") != want:
            errors.append(f"manifest: {a.get('id')}: sheet {a.get('sheet')!r}, want {want!r}")
    return m


def same_bytes(a, b):
    try:
        with open(a, "rb") as fa, open(b, "rb") as fb:
            return fa.read() == fb.read()
    except OSError:
        return False


def main(argv):
    if not argv:
        print(__doc__.strip(), file=sys.stderr)
        return 2
    root, copies = argv[0], argv[1:]
    errors = []
    m = check_manifest(root, errors)
    names = ["manifest.json"]
    if m is not None:
        for a in m["animals"]:
            aid = a.get("id")
            path = os.path.join(root, str(a.get("sheet")))
            names.append(str(a.get("sheet")))
            if not os.path.isfile(path):
                errors.append(f"{aid}: missing sheet {path}")
                continue
            check_sheet(path, aid, errors)
    for copy in copies:
        for name in names:
            if not same_bytes(os.path.join(root, name), os.path.join(copy, name)):
                errors.append(f"copy: {os.path.join(copy, name)} is missing or differs from "
                              f"{os.path.join(root, name)}")
    if errors:
        for e in errors:
            print(f"FAIL {e}", file=sys.stderr)
        print(f"{len(errors)} problem(s) in {root}", file=sys.stderr)
        return 1
    print(f"OK {root}: {len(names) - 1} sheets, {FRAMES} frames each"
          + (f"; {len(copies)} byte-identical copies" if copies else ""))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
