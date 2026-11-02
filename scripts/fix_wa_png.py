#!/usr/bin/env python3
"""Repack any author PNG as WA-compatible 8-bit indexed map (black air, ≤64 cols, ÷8)."""
from __future__ import annotations

import argparse
import struct
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[1]


def is_air(r: int, g: int, b: int, a: int = 255) -> bool:
    if a <= 8:
        return True
    return r <= 16 and g <= 16 and b <= 16


def pad_div8(n: int) -> int:
    return ((n + 7) // 8) * 8


def quantize_wa(im_rgb: Image.Image) -> Image.Image:
    w, h = im_rgb.size
    src = im_rgb.load()
    air = bytearray(w * h)
    for y in range(h):
        for x in range(w):
            r, g, b = src[x, y]
            air[y * w + x] = 1 if is_air(r, g, b) else 0
    q = im_rgb.quantize(colors=64, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE)
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
    return out


def rgba_to_rgb_on_black(im: Image.Image) -> Image.Image:
    im = im.convert("RGBA")
    bg = Image.new("RGB", im.size, (0, 0, 0))
    bg.paste(im, mask=im.split()[3])
    return bg


def png_info(path: Path) -> str:
    b = path.read_bytes()
    pos = 8
    w = h = bd = ct = 0
    while pos < len(b):
        ln = int.from_bytes(b[pos : pos + 4], "big")
        pos += 4
        chunk = b[pos : pos + 4]
        pos += 4
        if chunk == b"IHDR":
            w, h = struct.unpack(">II", b[pos : pos + 8])
            bd, ct = b[pos + 8], b[pos + 9]
            break
        pos += ln + 4
    return f"{w}x{h} bitdepth={bd} color_type={ct} bytes={path.stat().st_size}"


def convert(in_path: Path, out_path: Path) -> None:
    im = Image.open(in_path)
    w, h = im.size
    pw, ph = pad_div8(w), pad_div8(h)
    rgb = rgba_to_rgb_on_black(im)
    if (pw, ph) != (w, h):
        padded = Image.new("RGB", (pw, ph), (0, 0, 0))
        padded.paste(rgb, (0, 0))
        rgb = padded
    wa = quantize_wa(rgb)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    wa.save(out_path, optimize=True)
    pal = wa.getpalette() or []
    ncols = len({i for i in wa.getdata() if i != 0}) + 1
    print(f"IN  {png_info(in_path)} mode={im.mode}")
    print(f"OUT {png_info(out_path)} mode=P palette_used≈{ncols}")


def main() -> None:
    ap = argparse.ArgumentParser(description="Convert PNG to WA indexed map")
    ap.add_argument("input", type=Path)
    ap.add_argument("-o", "--output", type=Path, required=True)
    args = ap.parse_args()
    convert(args.input, args.output)


if __name__ == "__main__":
    main()
