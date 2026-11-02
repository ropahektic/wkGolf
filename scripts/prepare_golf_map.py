#!/usr/bin/env python3
"""Prepare author PNG for WA + wkGolf.

Author color key (inside each red rectangle = one hole):
  - Red border  (136, 0, 21)  — cell bounds; do not treat as equal grid tiles
  - Yellow tee  (235, 210, 55) — spawn / drop column
  - Blue cup    (0, 162, 232)  — hole target marker (stripped before play)
  - Green       (60, 120, 50)  — fairway
  - Brown       (90, 70, 40)   — dirt
  - Orange      (255, 127, 39) — lava (ignored for now)
  - Black       (0, 0, 0)      — air
"""
from __future__ import annotations

import json
import struct
import textwrap
from collections import deque
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
IN_RAW = ROOT / "incoming" / "GolfTest1_raw.png"
OUT_PNG = ROOT / "dist" / "GolfTest1.png"
OUT_TOML = ROOT / "golf.toml"

PLAY_ROWS = 2
COLS = 3
PARS = [2, 2, 2, 4, 3, 3]
CUP_RADIUS = 22.0

# Horizontal red dividers between the three map rows (from border scan).
ROW_INNER = [(5, 1094), (1103, 2222), (2231, 3606)]


def is_border(r: int, g: int, b: int, a: int = 255) -> bool:
    return a > 200 and r > 100 and g < 40 and b < 40 and r > g + 60 and r > b + 60


def is_blue(r: int, g: int, b: int, a: int = 255) -> bool:
    return a > 200 and r == 0 and g == 162 and b == 232


def is_tee(r: int, g: int, b: int, a: int = 255) -> bool:
    return a > 200 and r == 235 and g == 210 and b == 55


def pad_div8(w: int, h: int) -> tuple[int, int]:
    return ((w + 7) // 8) * 8, ((h + 7) // 8) * 8


def connected_components(points: set[tuple[int, int]]) -> list[dict]:
    seen = set(points)
    comps: list[dict] = []
    for start in points:
        if start not in seen:
            continue
        comp: list[tuple[int, int]] = []
        q = deque([start])
        seen.remove(start)
        while q:
            x, y = q.popleft()
            comp.append((x, y))
            for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                nb = (x + dx, y + dy)
                if nb in seen:
                    seen.remove(nb)
                    q.append(nb)
        xs = [p[0] for p in comp]
        ys = [p[1] for p in comp]
        comps.append(
            {
                "n": len(comp),
                "cx": sum(xs) / len(xs),
                "cy": sum(ys) / len(ys),
                "xmin": min(xs),
                "xmax": max(xs),
                "ymin": min(ys),
                "ymax": max(ys),
            }
        )
    return comps


def vertical_border_segments(px, y: int, width: int) -> list[tuple[int, int]]:
    segs: list[tuple[int, int]] = []
    x = 0
    while x < width:
        while x < width and not is_border(*px[x, y]):
            x += 1
        if x >= width:
            break
        x0 = x
        while x < width and is_border(*px[x, y]):
            x += 1
        segs.append((x0, x - 1))
    return segs


def cells_for_row(px, row: int, width: int) -> list[dict]:
    top, bottom = ROW_INNER[row]
    y_mid = (top + bottom) // 2
    segs = vertical_border_segments(px, y_mid, width)
    if len(segs) < 4:
        raise SystemExit(f"row {row}: expected 4 vertical border bands, got {segs}")

    cells: list[dict] = []
    for col in range(COLS):
        left = segs[col][1] + 1
        right = segs[col + 1][0] - 1
        cells.append(
            {
                "row": row,
                "col": col,
                "left": left,
                "right": right,
                "top": top,
                "bottom": bottom,
                "origin_x": float(left),
                "origin_y": float(top),
            }
        )
    return cells


def markers_in_cell(im: Image.Image, cell: dict) -> tuple[dict, dict]:
    px = im.load()
    l, r, t, b = cell["left"], cell["right"], cell["top"], cell["bottom"]
    blue_pts: set[tuple[int, int]] = set()
    tee_pts: set[tuple[int, int]] = set()
    for y in range(t, b + 1):
        for x in range(l, r + 1):
            c = px[x, y]
            if is_blue(*c):
                blue_pts.add((x, y))
            elif is_tee(*c):
                tee_pts.add((x, y))

    blue_comps = connected_components(blue_pts)
    tee_comps = connected_components(tee_pts)
    if len(blue_comps) != 1:
        hole = cell["row"] * COLS + cell["col"] + 1
        raise SystemExit(
            f"hole {hole}: expected exactly one blue cup marker, found {len(blue_comps)}"
        )
    if len(tee_comps) != 1:
        hole = cell["row"] * COLS + cell["col"] + 1
        raise SystemExit(
            f"hole {hole}: expected exactly one yellow tee pad, found {len(tee_comps)}"
        )
    return blue_comps[0], tee_comps[0]


def to_local(cell: dict, cx: float, cy: float) -> tuple[float, float]:
    return cx - cell["origin_x"], cy - cell["origin_y"]


def cup_local(blue: dict, cell: dict) -> tuple[float, float]:
    ox = cell["origin_x"]
    oy = cell["origin_y"]
    cup_x = (blue["xmin"] + blue["xmax"]) / 2.0 - ox
    lip_y = blue["ymin"] - oy
    cup_y = lip_y + CUP_RADIUS * 1.2
    return round(cup_x, 1), round(cup_y, 1)


def detect_holes(im: Image.Image) -> list[dict]:
    px = im.load()
    w, h = im.size
    holes: list[dict] = []
    for row in range(PLAY_ROWS):
        for cell in cells_for_row(px, row, w):
            hole_n = row * COLS + cell["col"] + 1
            blue, tee = markers_in_cell(im, cell)
            tee_x, tee_y = to_local(cell, tee["cx"], tee["cy"])
            cup_x, cup_y = cup_local(blue, cell)
            holes.append(
                {
                    "hole": hole_n,
                    "origin_x": int(cell["origin_x"]),
                    "origin_y": int(cell["origin_y"]),
                    "tee_x": round(tee_x, 1),
                    "tee_y": round(tee_y, 1),
                    "cup_x": cup_x,
                    "cup_y": cup_y,
                    "radius": CUP_RADIUS,
                    "par": PARS[hole_n - 1],
                    "cell_w": cell["right"] - cell["left"] + 1,
                    "cell_h": cell["bottom"] - cell["top"] + 1,
                }
            )
    return holes


def replace_blue_markers(im: Image.Image) -> int:
    rgb = im.convert("RGB")
    px = rgb.load()
    w, h = rgb.size
    src = im.convert("RGBA")
    spx = src.load()
    removed = 0
    for y in range(h):
        for x in range(w):
            if not is_blue(*spx[x, y]):
                continue
            removed += 1
            for dy in (1, 0, -1, 2, -2):
                ny = y + dy
                if ny < 0 or ny >= h:
                    continue
                nr, ng, nb = px[x, ny]
                if not is_blue(nr, ng, nb):
                    px[x, y] = (nr, ng, nb)
                    break
            else:
                px[x, y] = (0, 0, 0)
    return removed


def quantize_wa(im_rgb: Image.Image) -> Image.Image:
    w, h = im_rgb.size
    air = bytearray(w * h)
    px = im_rgb.load()
    for y in range(h):
        for x in range(w):
            r, g, b = px[x, y]
            air[y * w + x] = 1 if r <= 16 and g <= 16 and b <= 16 else 0

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


def write_toml(holes: list[dict], map_name: str = "GolfTest1.png") -> None:
    lines = [
        "# wkGolf — 6 playable holes on a 3x3 sheet (rows 1–2). Row 3 = TBD.",
        "# Each hole = red rectangle. Yellow tee, blue cup (blue stripped in shipped PNG).",
        f'map = "{map_name}"',
        "hud = true",
        "",
    ]
    for h in holes:
        lines.extend(
            [
                "[[holes]]",
                f"origin_x = {h['origin_x']}",
                f"origin_y = {h['origin_y']}",
                f"tee_x = {h['tee_x']}",
                f"tee_y = {h['tee_y']}",
                f"cup_x = {h['cup_x']}",
                f"cup_y = {h['cup_y']}",
                f"radius = {int(h['radius'])}",
                f"par = {h['par']}",
                "",
            ]
        )
    OUT_TOML.write_text("\n".join(lines), encoding="utf-8")


def pad_canvas(im: Image.Image) -> Image.Image:
    w, h = im.size
    pw, ph = pad_div8(w, h)
    if (pw, ph) == (w, h):
        return im
    out = Image.new("RGBA", (pw, ph), (0, 0, 0, 255))
    out.paste(im, (0, 0))
    return out


def main() -> None:
    if not IN_RAW.is_file():
        raise SystemExit(f"missing {IN_RAW} — drop the author PNG there first")

    src = Image.open(IN_RAW).convert("RGBA")
    holes = detect_holes(src)
    padded = pad_canvas(src)
    removed = replace_blue_markers(padded)
    wa = quantize_wa(padded.convert("RGB"))

    OUT_PNG.parent.mkdir(parents=True, exist_ok=True)
    wa.save(OUT_PNG, optimize=True)
    write_toml(holes)

    b = OUT_PNG.read_bytes()
    pos = 8
    iw = ih = 0
    while pos < len(b):
        ln = struct.unpack(">I", b[pos : pos + 4])[0]
        chunk = b[pos + 4 : pos + 8]
        if chunk == b"IHDR":
            iw, ih = struct.unpack(">II", b[pos + 8 : pos + 16])
            break
        pos += 12 + ln

    meta = {"png": str(OUT_PNG), "size": [iw, ih], "holes": holes, "blue_removed": removed}
    (ROOT / "dist" / "golf_map_meta.json").write_text(json.dumps(meta, indent=2), encoding="utf-8")
    print(f"OK {OUT_PNG} {iw}x{ih} bytes={len(b)} blue_removed={removed}")
    print(f"OK {OUT_TOML} holes={len(holes)}")
    for h in holes:
        print(
            f"  H{h['hole']} par={h['par']} cell={h['cell_w']}x{h['cell_h']} "
            f"origin=({h['origin_x']},{h['origin_y']}) "
            f"tee=({h['tee_x']},{h['tee_y']}) cup=({h['cup_x']},{h['cup_y']})"
        )


if __name__ == "__main__":
    main()
