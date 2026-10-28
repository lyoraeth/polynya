# Copyright (c) Danil Klimov (lyoraeth)
# SPDX-License-Identifier: GPL-3.0-or-later

# /// script
# dependencies = ["resvg-py", "pillow", "numpy"]
# ///
"""Builds derivatives of the mark from `assets/logo/mark.svg`.

    uv run tools/logo.py

Writes next to the source:
- `mark-grain.svg`: the same vector with grain (an SVG filter, stays vector);
- `polynya.ico`: exe and tray icon, 16–256 px; grain from 48 px, smaller sizes turn it into ripple;
- `mark-512.png`: a raster with grain for the README and previews.
"""
import io
import re
import struct
from pathlib import Path

import numpy as np
import resvg_py
from PIL import Image

ROOT = Path(__file__).resolve().parent.parent / "assets" / "logo"
SRC = ROOT / "mark.svg"
GRAIN = 0.08
GRAIN_FROM = 48
ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256]
# Icon margins: 1/ICON_PAD of the size on each side.
ICON_PAD = 8

# Vector grain: monochrome noise applied to the mark only.
FILTER = """<filter id="grain" filterUnits="userSpaceOnUse" x="0" y="0" width="43" height="45">
<feTurbulence type="fractalNoise" baseFrequency="7" numOctaves="2" seed="7" stitchTiles="stitch" result="noise"/>
<feColorMatrix in="noise" type="saturate" values="0" result="mono"/>
<feComposite in="mono" in2="SourceGraphic" operator="in" result="masked"/>
<feBlend in="masked" in2="SourceGraphic" mode="overlay" result="blend"/>
<feComposite in="blend" in2="SourceGraphic" operator="in"/>
</filter>"""


def grain_svg(svg: str) -> str:
    body_start = svg.index(">", svg.index("<svg")) + 1
    defs_at = svg.index("<defs>")
    body = svg[body_start:defs_at]
    rest = svg[defs_at:]
    rest = rest.replace("<defs>", "<defs>\n" + FILTER, 1)
    return svg[:body_start] + '\n<g filter="url(#grain)">' + body + "</g>\n" + rest


def render(size: int) -> Image.Image:
    png = resvg_py.svg_to_bytes(svg_path=str(SRC), width=size * 4, height=round(size * 4 * 45 / 43))
    big = Image.open(io.BytesIO(bytes(png))).convert("RGBA")
    # Square icon: the 43×45 mark centered.
    side = max(big.size)
    sq = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    sq.paste(big, ((side - big.width) // 2, (side - big.height) // 2))
    return sq


def grain(im: Image.Image, amount: float) -> Image.Image:
    a = np.asarray(im, dtype=np.float32)
    n = np.random.default_rng(7).normal(0.0, 255 * amount, a.shape[:2])[..., None]
    a[..., :3] = np.clip(a[..., :3] + n, 0, 255)
    return Image.fromarray(a.astype(np.uint8), "RGBA")


def icon(size: int) -> Image.Image:
    """Icon with 1/8 margins on each side (4 px at 32×32): without them the mark touches
    the edges in the tray and on the taskbar and looks bigger than neighboring icons."""
    pad = size // ICON_PAD
    inner = size - 2 * pad
    im = render(inner)
    if size >= GRAIN_FROM:
        im = grain(im, GRAIN)
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    canvas.paste(im.resize((inner, inner), Image.LANCZOS), (pad, pad))
    return canvas


def write_ico(path: Path, images: list[Image.Image]) -> None:
    """ICO with PNG inside: Windows Vista+ reads these at any size."""
    blobs = []
    for im in images:
        buf = io.BytesIO()
        im.save(buf, "PNG")
        blobs.append(buf.getvalue())
    head = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries = b""
    for im, data in zip(images, blobs):
        w, h = im.size
        entries += struct.pack("<BBBBHHII", w % 256, h % 256, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    path.write_bytes(head + entries + b"".join(blobs))


def main() -> None:
    svg = SRC.read_text(encoding="utf-8")
    (ROOT / "mark-grain.svg").write_text(grain_svg(svg), encoding="utf-8")
    write_ico(ROOT / "polynya.ico", [icon(s) for s in ICO_SIZES])
    grain(render(512), GRAIN).resize((512, 512), Image.LANCZOS).save(ROOT / "mark-512.png")
    print("ok:", ", ".join(p.name for p in sorted(ROOT.iterdir())))


if __name__ == "__main__":
    main()
