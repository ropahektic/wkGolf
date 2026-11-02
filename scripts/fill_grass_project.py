#!/usr/bin/env python3
"""Offline grass fill for wkGolf editor projects (.wkgolf.json).

Renders fairway placements and fills interior air gaps with tile body color.
Usage:
  python3 scripts/fill_grass_project.py incoming/golf_map.wkgolf.json -o dist/golf_map_filled.png
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
TILESET = ROOT / "editor" / "assets" / "tileset.png"
GRID_COLS = 7
GRID_ROWS = 4
FILLER_TILE = 22
GRASS_BODY = (42, 119, 70)


def is_air(r: int, g: int, b: int, a: int = 255) -> bool:
    return a <= 8 or (r <= 16 and g <= 16 and b <= 16)


def load_tiles() -> tuple[list[Image.Image], int, int, int]:
    sheet = Image.open(TILESET).convert("RGBA")
    tw, th = sheet.width // GRID_COLS, sheet.height // GRID_ROWS
    tiles: list[Image.Image] = []
    for row in range(GRID_ROWS):
        for col in range(GRID_COLS):
            idx = row * GRID_COLS + col
            tiles.append(sheet.crop((col * tw, row * th, (col + 1) * tw, (row + 1) * th)))
    left_pad = tw
    for i in range(1, len(tiles)):
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


def draw_project(data: dict, tiles: list[Image.Image], tw: int, th: int, snap_w: int) -> Image.Image:
    cols = int(data["mapCols"])
    height = int(data["mapHeight"])
    width = map_width(cols, snap_w, tw)
    im = Image.new("RGBA", (width, height), (0, 0, 0, 255))
    overlay_meta = len(tiles) > 28  # hole cup appended in editor
    for p in data.get("placements", []):
        idx = int(p["tileIndex"])
        if idx <= 0 or idx >= len(tiles):
            continue
        if idx >= 28:  # overlay / extras — skip for terrain fill pass
            continue
        x = int(p["x"]) if "x" in p else int(p["col"]) * snap_w
        y = int(p["y"])
        im.paste(tiles[idx], (x, y), tiles[idx])
    return im


def apply_grass_fill(im: Image.Image) -> tuple[Image.Image, int]:
    """Fill only enclosed air (not connected to top edge) — solid body colour."""
    w, h = im.size
    px = im.load()

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
            r, g, b, a = px[x, y]
            if not is_air(r, g, b, a) or exterior[y][x]:
                continue
            px[x, y] = (*GRASS_BODY, 255)
            filled += 1
    return im, filled


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("project", type=Path)
    ap.add_argument("-o", "--out", type=Path, required=True)
    args = ap.parse_args()
    data = json.loads(args.project.read_text(encoding="utf-8"))
    tiles, tw, th, snap_w = load_tiles()
    im = draw_project(data, tiles, tw, th, snap_w)
    im, n = apply_grass_fill(im)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    im.convert("RGB").save(args.out)
    print(f"OK {args.out} filled={n}px")


if __name__ == "__main__":
    main()
