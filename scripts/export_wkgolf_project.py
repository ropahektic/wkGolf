#!/usr/bin/env python3
"""Export wkGolf editor project → WA indexed PNG + golf.toml.

Conservative grass fill: solid body colour inside enclosed air only (no sky flood).
"""
from __future__ import annotations

import argparse
import json
import struct
from collections import deque
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
EDITOR = ROOT / "editor" / "assets"
TILESET = EDITOR / "tileset.png"
GIRDER = EDITOR / "wall-girder.png"
HOLE = EDITOR / "hole-cup.png"

GRID_COLS = 7
GRID_ROWS = 4
FAIRWAY_COUNT = GRID_COLS * GRID_ROWS
TEE_RGB = (235, 210, 55)
TEE_PAD_W = 12
TEE_PAD_H = 48
CUP_RADIUS = 22
GRASS_BODY = (42, 119, 70)
HOLE_TILE = 29
GIRDER_TILE = 28


def is_air(r: int, g: int, b: int, a: int = 255) -> bool:
    return a <= 8 or (r <= 16 and g <= 16 and b <= 16)


def pad_div8(n: int) -> int:
    return ((n + 7) // 8) * 8


def load_assets() -> tuple[list[Image.Image], int, int, int]:
    sheet = Image.open(TILESET).convert("RGBA")
    tw, th = sheet.width // GRID_COLS, sheet.height // GRID_ROWS
    tiles: list[Image.Image] = []
    for row in range(GRID_ROWS):
        for col in range(GRID_COLS):
            tiles.append(sheet.crop((col * tw, row * th, (col + 1) * tw, (row + 1) * th)))
    tiles.append(Image.open(GIRDER).convert("RGBA"))
    tiles.append(Image.open(HOLE).convert("RGBA"))
    left_pad = tw
    for i in range(1, FAIRWAY_COUNT):
        t = tiles[i]
        px = t.load()
        for x in range(tw):
            for y in range(th):
                if px[x, y][3] > 8:
                    left_pad = min(left_pad, x)
                    break
    snap_w = max(1, tw - left_pad)
    return tiles, tw, th, snap_w


def map_width(cols: int, snap_w: int, tile_w: int) -> int:
    if cols <= 0:
        return tile_w
    return (cols - 1) * snap_w + tile_w


def placement_xy(p: dict, snap_w: int) -> tuple[int, int]:
    if "x" in p:
        return int(p["x"]), int(p["y"])
    return int(p["col"]) * snap_w, int(p["y"])


def cup_geom_from_hole_tile(tile: Image.Image, px: int, py: int) -> dict:
    w, h = tile.size
    data = tile.load()
    xmin, ymin, xmax, ymax = w, h, -1, -1
    sx = sy = n = 0
    for y in range(h):
        for x in range(w):
            r, g, b, a = data[x, y]
            if is_air(r, g, b, a):
                xmin = min(xmin, x)
                ymin = min(ymin, y)
                xmax = max(xmax, x)
                ymax = max(ymax, y)
                sx += x
                sy += y
                n += 1
    if n == 0:
        return {"cx": px + w / 2, "cy": py + h / 2, "lip_y": py + h / 2}
    lip_y = py + ymin
    cx = px + sx / n
    cy = lip_y + CUP_RADIUS * 1.2
    return {"cx": cx, "cy": cy, "lip_y": lip_y}


def apply_enclosed_grass_fill(im: Image.Image) -> int:
    """Fill only air pockets not connected to the top edge — solid colour, no tiling."""
    w, h = im.size
    px = im.load()
    n = w * h

    def air(x: int, y: int) -> bool:
        r, g, b, a = px[x, y]
        return is_air(r, g, b, a)

    exterior = [[False] * w for _ in range(h)]
    q: deque[tuple[int, int]] = deque()
    for x in range(w):
        if air(x, 0):
            exterior[0][x] = True
            q.append((x, 0))
    while q:
        x, y = q.popleft()
        for nx, ny in ((x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)):
            if 0 <= nx < w and 0 <= ny < h and not exterior[ny][nx] and air(nx, ny):
                exterior[ny][nx] = True
                q.append((nx, ny))

    filled = 0
    for y in range(h):
        for x in range(w):
            if not air(x, y) or exterior[y][x]:
                continue
            px[x, y] = (*GRASS_BODY, 255)
            filled += 1
    return filled


def draw_tee(im: Image.Image, x: int, y: int) -> None:
    draw = Image.new("RGBA", im.size, (0, 0, 0, 0))
    d = Image.new("RGBA", (TEE_PAD_W, TEE_PAD_H), (*TEE_RGB, 255))
    ox = int(round(x - TEE_PAD_W / 2))
    oy = int(round(y - TEE_PAD_H / 2))
    im.paste(d, (ox, oy), d)


def render_project(data: dict, tiles: list[Image.Image], tw: int, th: int, snap_w: int, *, grass_fill: bool) -> Image.Image:
    cols = int(data["mapCols"])
    height = int(data["mapHeight"])
    width = map_width(cols, snap_w, tw)
    im = Image.new("RGBA", (width, height), (0, 0, 0, 255))
    placements = data.get("placements", [])
    terrain = [p for p in placements if int(p["tileIndex"]) not in (HOLE_TILE,)]
    overlays = [p for p in placements if int(p["tileIndex"]) == HOLE_TILE]

    for p in sorted(terrain, key=lambda q: q["y"]):
        idx = int(p["tileIndex"])
        if idx <= 0 or idx >= len(tiles):
            continue
        x, y = placement_xy(p, snap_w)
        im.paste(tiles[idx], (x, y), tiles[idx])

    if grass_fill:
        apply_enclosed_grass_fill(im)

    for p in overlays:
        x, y = placement_xy(p, snap_w)
        im.paste(tiles[HOLE_TILE], (x, y), tiles[HOLE_TILE])

    for t in data.get("tees", []):
        draw_tee(im, int(t["x"]), int(t["y"]))

    return im


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


def build_toml(data: dict, tiles: list[Image.Image], snap_w: int, map_name: str) -> str:
    par = int(data.get("defaultPar", 3))
    tees = sorted(data.get("tees", []), key=lambda t: t["x"])
    hole_tile = tiles[HOLE_TILE]
    cups = []
    for p in data.get("placements", []):
        if int(p["tileIndex"]) != HOLE_TILE:
            continue
        x, y = placement_xy(p, snap_w)
        geom = cup_geom_from_hole_tile(hole_tile, x, y)
        cups.append({"x": x, "geom": geom})
    cups.sort(key=lambda c: c["geom"]["cx"])
    n = min(len(tees), len(cups))
    lines = [
        "# wkGolf — exported from wkgolf editor project",
        f'map = "{map_name}"',
        "hud = true",
        "",
    ]
    for i in range(n):
        tee = tees[i]
        cup = cups[i]["geom"]
        lines += [
            "[[holes]]",
            "origin_x = 0",
            "origin_y = 0",
            f"tee_x = {float(tee['x']):.1f}",
            f"tee_y = {float(tee['y']):.1f}",
            f"cup_x = {cup['cx']:.1f}",
            f"cup_y = {cup['cy']:.1f}",
            f"radius = {CUP_RADIUS}",
            f"par = {par}",
            "",
        ]
    return "\n".join(lines)


def export_png(im: Image.Image, out: Path) -> tuple[int, int]:
    w, h = im.size
    pw, ph = pad_div8(w), pad_div8(h)
    padded = Image.new("RGBA", (pw, ph), (0, 0, 0, 255))
    padded.paste(im, (0, 0))
    wa = quantize_wa(padded.convert("RGB"))
    out.parent.mkdir(parents=True, exist_ok=True)
    wa.save(out, optimize=True)
    return pw, ph


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("project", type=Path)
    ap.add_argument("--out-dir", type=Path, default=ROOT / "dist")
    ap.add_argument("--map-name", default="golf_map.png")
    ap.add_argument("--grass-fill", action="store_true", help="Fill enclosed air pockets only")
    ap.add_argument("--preview", type=Path, default=None, help="RGBA preview PNG")
    args = ap.parse_args()

    data = json.loads(args.project.read_text(encoding="utf-8"))
    tiles, tw, th, snap_w = load_assets()
    im = render_project(data, tiles, tw, th, snap_w, grass_fill=args.grass_fill)
    if args.preview:
        im.save(args.preview)
    out_png = args.out_dir / args.map_name
    pw, ph = export_png(im, out_png)
    toml = build_toml(data, tiles, snap_w, args.map_name)
    out_toml = args.out_dir / "golf.toml"
    out_toml.write_text(toml, encoding="utf-8")
    print(f"OK {out_png} {pw}x{ph}")
    print(f"OK {out_toml} holes={toml.count('[[holes]]')}")


if __name__ == "__main__":
    main()
