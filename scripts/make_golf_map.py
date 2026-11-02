#!/usr/bin/env python3
"""Side-view wkGolf map: each hole in a 960x640 cell (air headroom for high shots)."""
from __future__ import annotations

import struct
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "dist" / "GolfTest1.png"

CELL_W = 960
CELL_H = 640
HOLES = 2
W = CELL_W * HOLES
H = CELL_H

SURFACE_Y = 340
FAIRWAY_BOTTOM = 400
TEE_PAD_LEFT = 80
TEE_PAD_RIGHT = 160
TEE_X = (TEE_PAD_LEFT + TEE_PAD_RIGHT) // 2
CUP_X = 720
CUP_RADIUS = 22


def draw_hole(draw: ImageDraw.ImageDraw, ox: int) -> None:
    draw.rectangle([ox, FAIRWAY_BOTTOM, ox + CELL_W, H], fill=(90, 70, 40))
    draw.rectangle([ox + 60, SURFACE_Y, ox + CELL_W - 40, FAIRWAY_BOTTOM], fill=(60, 120, 50))
    draw.rectangle(
        [ox + TEE_PAD_LEFT, SURFACE_Y, ox + TEE_PAD_RIGHT, FAIRWAY_BOTTOM],
        fill=(235, 210, 55),
    )
    cup_top = SURFACE_Y - 2
    cup_bottom = SURFACE_Y + CUP_RADIUS * 2 + 8
    cx = ox + CUP_X
    draw.ellipse(
        [cx - CUP_RADIUS, cup_top, cx + CUP_RADIUS, cup_bottom],
        fill=(0, 0, 0),
    )
    draw.rectangle(
        [cx - CUP_RADIUS, cup_top, cx + CUP_RADIUS, SURFACE_Y + 10],
        fill=(0, 0, 0),
    )


def make_map() -> None:
    im = Image.new("RGB", (W, H), (0, 0, 0))
    draw = ImageDraw.Draw(im)
    for i in range(HOLES):
        draw_hole(draw, i * CELL_W)

    w, h = im.size
    w -= w % 8
    h -= h % 8
    im = im.crop((0, 0, w, h))
    rgb = im.convert("RGB")
    air = bytearray(w * h)
    px = rgb.load()
    for y in range(h):
        for x in range(w):
            r, g, b = px[x, y]
            is_air = r <= 16 and g <= 16 and b <= 16
            air[y * w + x] = 1 if is_air else 0

    q = rgb.quantize(colors=64, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE)
    pal_flat = q.getpalette() or []
    ncol = len(pal_flat) // 3
    colors = [(pal_flat[i * 3], pal_flat[i * 3 + 1], pal_flat[i * 3 + 2]) for i in range(ncol)]
    black_idx = next((i for i, c in enumerate(colors) if c == (0, 0, 0)), None)
    new_colors = [(0, 0, 0)] + [c for i, c in enumerate(colors) if i != black_idx][:63]
    old_to_new: dict[int, int] = {}
    for i, c in enumerate(colors):
        if c == (0, 0, 0):
            old_to_new[i] = 0
        else:
            try:
                old_to_new[i] = new_colors.index(c)
            except ValueError:
                old_to_new[i] = 0
    indices = list(q.getdata())
    for i, v in enumerate(indices):
        indices[i] = 0 if air[i] else old_to_new.get(v, 0)
    flat: list[int] = []
    for c in new_colors:
        flat.extend(c)
    while len(flat) < 256 * 3:
        flat.extend((0, 0, 0))
    out = Image.new("P", (w, h))
    out.putpalette(flat)
    out.putdata(indices)
    OUT.parent.mkdir(parents=True, exist_ok=True)
    out.save(OUT, optimize=True)
    b = OUT.read_bytes()
    pos = 8
    while pos < len(b):
        ln = struct.unpack(">I", b[pos : pos + 4])[0]
        chunk = b[pos + 4 : pos + 8]
        if chunk == b"IHDR":
            iw, ih = struct.unpack(">II", b[pos + 8 : pos + 16])
            print(f"OK {OUT} {iw}x{ih} bytes={len(b)} holes={HOLES} cell={CELL_W}x{CELL_H}")
            break
        pos += 12 + ln


if __name__ == "__main__":
    make_map()
