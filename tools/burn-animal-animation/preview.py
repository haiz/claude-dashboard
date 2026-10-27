"""Review strips: every frame of each sheet at the gauge sizes, on light and dark.

Run with any Python 3 that has Pillow (not Blender):
    python3 -I preview.py <sheet-dir> <out-dir>

Writes <id>_<size>_<theme>.png (one strip of all 24 frames, 52 px and 68 px
cells, light #f5f5f7 and dark #1e1e1e) and <id>_review.png stacking the four.
"""

import json
import os
import sys

from PIL import Image

SIZES = (52, 68)
THEMES = (("light", (0xF5, 0xF5, 0xF7)), ("dark", (0x1E, 0x1E, 0x1E)))
GAP = 4


def frames(sheet, m):
    cw, ch = m["cellWidth"], m["cellHeight"]
    for i in range(m["frameCount"]):
        r, c = divmod(i, m["columns"])
        yield sheet.crop((c * cw, r * ch, (c + 1) * cw, (r + 1) * ch))


def strip(cells, size, bg):
    n = len(cells)
    out = Image.new("RGB", (n * (size + GAP) + GAP, size + 2 * GAP), bg)
    for i, cell in enumerate(cells):
        small = cell.convert("RGBa").resize((size, size), Image.LANCZOS).convert("RGBA")
        out.paste(small, (GAP + i * (size + GAP), GAP), small)
    return out


def main():
    src, dst = sys.argv[1], sys.argv[2]
    os.makedirs(dst, exist_ok=True)
    with open(os.path.join(src, "manifest.json"), encoding="utf-8") as fh:
        m = json.load(fh)
    for entry in m["animals"]:
        sheet = Image.open(os.path.join(src, entry["sheet"])).convert("RGBA")
        cells = list(frames(sheet, m))
        strips = []
        for size in SIZES:
            for theme, bg in THEMES:
                s = strip(cells, size, bg)
                s.save(os.path.join(dst, f"{entry['id']}_{size}_{theme}.png"))
                strips.append(s)
        w = max(s.width for s in strips)
        review = Image.new("RGB", (w, sum(s.height for s in strips)), (128, 128, 128))
        y = 0
        for s in strips:
            review.paste(s, (0, y))
            y += s.height
        review.save(os.path.join(dst, f"{entry['id']}_review.png"))
        print(os.path.join(dst, f"{entry['id']}_review.png"))


if __name__ == "__main__":
    main()
