# Copyright (c) Danil Klimov (lyoraeth)
# SPDX-License-Identifier: GPL-3.0-or-later

# /// script
# dependencies = ["resvg-py", "pillow", "numpy"]
# ///
"""ASCII version of the mark: characters are matched by shape, each colored from the mark's gradient.

    uv run tools/ascii.py [columns] [--preview path.png]

Writes `assets/logo/mark-ascii.json`: `{"lines": [...], "colors": [[...]]}` with a `#rrggbb`
color per character (an empty string for spaces). Read by "About this PC" and the CLI.
"""
import io
import json
import sys
from pathlib import Path

import numpy as np
import resvg_py
from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parent.parent / "assets" / "logo"
FONT = ImageFont.truetype(r"C:\Windows\Fonts\consola.ttf", 20)
CW = round(FONT.getlength("M"))
CH = sum(FONT.getmetrics())
# Characters the mark is built from: lines and solid fill.
CHARS = " .,:;'`-_=+/\\|()<>^"
COVER_MIN = 0.25
# A nearly full cell gets a flat fill instead of matching: matching produced noisy "@".
FULL = 0.55
FILL_FRONT = "#"
FILL_BACK = "+"


def glyph(c: str) -> np.ndarray:
    im = Image.new("L", (CW, CH), 0)
    ImageDraw.Draw(im).text((0, 0), c, fill=255, font=FONT)
    return np.asarray(im.filter(ImageFilter.GaussianBlur(1.6)), dtype=np.float32) / 255.0


GLYPHS = [(c, glyph(c)) for c in CHARS]


def build(cols: int) -> dict:
    rows = round(cols * CW / CH * 45 / 43)
    w, h = cols * CW, rows * CH
    png = resvg_py.svg_to_bytes(svg_path=str(ROOT / "mark.svg"), width=w, height=h)
    # resvg keeps the mark's aspect ratio; fit it to the character grid exactly.
    im = np.asarray(Image.open(io.BytesIO(bytes(png))).convert("RGBA").resize((w, h), Image.LANCZOS), dtype=np.float32) / 255.0
    alpha = im[..., 3]
    # Where the front stripes are: the back diagonal gets its own character, so depth
    # reads through shape, not only color.
    svg = (ROOT / "mark.svg").read_text(encoding="utf-8")
    # The back diagonal is the <use> elements; without them only the front stripes remain.
    front_svg = "\n".join(l for l in svg.splitlines() if not l.startswith("<use"))
    fp =resvg_py.svg_to_bytes(svg_string=front_svg, width=w, height=h)
    front = np.asarray(Image.open(io.BytesIO(bytes(fp))).convert("RGBA").resize((w, h), Image.LANCZOS), dtype=np.float32)[..., 3] / 255.0
    shape = np.asarray(Image.fromarray((alpha * 255).astype(np.uint8)).filter(ImageFilter.GaussianBlur(1.6)), dtype=np.float32) / 255.0
    lines, colors = [], []
    for r in range(rows):
        line, cl = "", []
        for c in range(cols):
            ys, xs = slice(r * CH, (r + 1) * CH), slice(c * CW, (c + 1) * CW)
            cover = alpha[ys, xs].mean()
            if cover < COVER_MIN:
                line += " "
                cl.append("")
                continue
            patch = shape[ys, xs]
            if cover > FULL:
                ch = FILL_FRONT if front[ys, xs].mean() > 0.5 else FILL_BACK
            else:
                ch = min(GLYPHS[1:], key=lambda g: float(((patch - g[1]) ** 2).sum()))[0]
            a = alpha[ys, xs][..., None]
            rgb = (im[ys, xs, :3] * a).sum(axis=(0, 1)) / max(a.sum(), 1e-6)
            line += ch
            cl.append("#%02x%02x%02x" % tuple(int(round(v * 255)) for v in rgb))
        lines.append(line)
        colors.append(cl)
    # Drop empty edge rows and the common left indent.
    while lines and not lines[-1].strip():
        lines.pop(), colors.pop()
    while lines and not lines[0].strip():
        lines.pop(0), colors.pop(0)
    left = min(len(l) - len(l.lstrip(" ")) for l in lines if l.strip())
    lines = [l[left:].rstrip() for l in lines]
    colors = [c[left:left + len(l)] for c, l in zip(colors, lines)]
    return {"lines": lines, "colors": colors}


def preview(art: dict, path: Path) -> None:
    w = max(len(l) for l in art["lines"]) * CW + 32
    im = Image.new("RGB", (w, len(art["lines"]) * CH + 32), (0x18, 0x17, 0x1c))
    d = ImageDraw.Draw(im)
    for i, (l, cs) in enumerate(zip(art["lines"], art["colors"])):
        for j, (ch, col) in enumerate(zip(l, cs)):
            if ch != " ":
                d.text((16 + j * CW, 16 + i * CH), ch, fill=col, font=FONT)
    im.save(path)


def main() -> None:
    args = sys.argv[1:]
    cols = int(args[0]) if args and args[0].isdigit() else 20
    art = build(cols)
    (ROOT / "mark-ascii.json").write_text(json.dumps(art, ensure_ascii=False), encoding="utf-8", newline="\n")
    if "--preview" in args:
        preview(art, Path(args[args.index("--preview") + 1]))
    print("\n".join(art["lines"]))


if __name__ == "__main__":
    main()
