//! Live stock WA landscape solid fill (Landscape+0x910) — read-only snapshot.
//!
//! BitGrid__Init (0x4F6370): +0x0C=bpp, +0x10=stride, +0x14=width, +0x18=height.
//! Map planes: ecx=8, push width, edi=height → stride≈width.
//! PNG custom maps are bpp=8 byte planes (non-zero = solid). We still score
//! byte vs bit decode with a worm-feet probe so a wrong unpack cannot win.

use crate::wa_mem;

/// Dense collision grid mirroring Engine `terrain.collision` (non-zero = solid).
#[derive(Clone, Debug)]
pub struct LandscapeMask {
    pub width: usize,
    pub height: usize,
    pub solid: Vec<u8>,
    pub solids: u32,
    pub bpp: u32,
    pub stride: u32,
    pub decode: &'static str,
    pub feet_score: i32,
}

impl LandscapeMask {
    #[inline]
    pub fn is_solid(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 {
            return false;
        }
        let ux = x as usize;
        let uy = y as usize;
        if ux >= self.width || uy >= self.height {
            return false;
        }
        self.solid[uy * self.width + ux] != 0
    }

    pub fn solid_near(&self, x: f64, y: f64, radius: i32) -> bool {
        let cx = x.round() as i32;
        let cy = y.round() as i32;
        let r = radius.max(0);
        for dy in -r..=r {
            for dx in -r..=r {
                if dx * dx + dy * dy <= r * r && self.is_solid(cx + dx, cy + dy) {
                    return true;
                }
            }
        }
        false
    }

    pub fn in_bounds(&self, x: f64, y: f64) -> bool {
        x >= 0.0 && y >= 0.0 && (x as usize) < self.width && (y as usize) < self.height
    }

    /// Top solid row in column `x`, if any.
    pub fn surface_y_at(&self, x: i32) -> Option<i32> {
        if x < 0 || x >= self.width as i32 {
            return None;
        }
        for y in 0..self.height as i32 {
            if self.is_solid(x, y) {
                return Some(y);
            }
        }
        None
    }

    /// Standing worm: body air, dirt a few px below. Higher = better decode.
    pub fn feet_probe_score(&self, worm_x: f64, worm_y: f64) -> i32 {
        let wx = worm_x.round() as i32;
        let wy = worm_y.round() as i32;
        let mut score = 0i32;
        if !self.is_solid(wx, wy) {
            score += 3;
        } else {
            score -= 4;
        }
        if !self.is_solid(wx, wy - 6) {
            score += 1;
        }
        let mut found_ground = false;
        for dy in 4..32 {
            if self.is_solid(wx, wy + dy) {
                found_ground = true;
                score += 10;
                break;
            }
        }
        if !found_ground {
            score -= 8;
        }
        score
    }
}

const OFF_WORLD_LANDSCAPE: usize = 0x20;
const OFF_WORLD_MAP_W: usize = 0x77c0;
const OFF_WORLD_MAP_H: usize = 0x77c4;
const OFF_LAND_SOLID_GRID: usize = 0x910;

const OFF_GRID_BITS: usize = 0x08;
const OFF_GRID_BPP: usize = 0x0c;
const OFF_GRID_STRIDE: usize = 0x10;
const OFF_GRID_W: usize = 0x14;
const OFF_GRID_H: usize = 0x18;
/// Stock/custom WA maps — golf PNG is 17888×512; do not cap at 4096.
const MAX_MAP_W: i32 = 65536;
const MAX_MAP_H: i32 = 8192;

fn decode_bytes(packed: &[u8], stride: usize, width: usize, height: usize) -> (Vec<u8>, u32) {
    let mut solid = vec![0u8; width * height];
    let mut solids = 0u32;
    for y in 0..height {
        let row = y * stride;
        for x in 0..width {
            if row + x < packed.len() && packed[row + x] != 0 {
                solid[y * width + x] = 1;
                solids += 1;
            }
        }
    }
    (solid, solids)
}

fn decode_bits(packed: &[u8], stride: usize, width: usize, height: usize) -> (Vec<u8>, u32) {
    let mut solid = vec![0u8; width * height];
    let mut solids = 0u32;
    for y in 0..height {
        let row = y * stride;
        for x in 0..width {
            let bi = row + (x >> 3);
            if bi >= packed.len() {
                continue;
            }
            if (packed[bi] >> (x & 7)) & 1 != 0 {
                solid[y * width + x] = 1;
                solids += 1;
            }
        }
    }
    (solid, solids)
}

fn build_mask(
    packed: &[u8],
    stride: usize,
    width: usize,
    height: usize,
    bpp: u32,
    worm_x: Option<(f64, f64)>,
) -> Result<LandscapeMask, &'static str> {
    let (byte_solid, byte_solids) = decode_bytes(packed, stride, width, height);
    let (bit_solid, bit_solids) = decode_bits(packed, stride, width, height);
    if byte_solids < 64 && bit_solids < 64 {
        return Err("mask nearly empty");
    }

    let mut byte_m = LandscapeMask {
        width,
        height,
        solid: byte_solid,
        solids: byte_solids,
        bpp,
        stride: stride as u32,
        decode: "bytes",
        feet_score: 0,
    };
    let mut bit_m = LandscapeMask {
        width,
        height,
        solid: bit_solid,
        solids: bit_solids,
        bpp,
        stride: stride as u32,
        decode: "bits",
        feet_score: 0,
    };

    // Custom PNG / byte-plane maps (bpp≥8): never bit-decode — that invents air solids.
    if bpp >= 8 {
        if byte_solids >= 64 {
            return Ok(byte_m);
        }
        return Err("byte mask nearly empty");
    }

    if let Some((wx, wy)) = worm_x {
        byte_m.feet_score = byte_m.feet_probe_score(wx, wy);
        bit_m.feet_score = bit_m.feet_probe_score(wx, wy);
        // Prefer the decode that looks like a standing worm on dirt.
        if bit_m.feet_score > byte_m.feet_score + 1 && bit_solids >= 64 {
            return Ok(bit_m);
        }
        if byte_solids >= 64 {
            return Ok(byte_m);
        }
        return Ok(bit_m);
    }

    // No worm yet: bpp=8 / stride≈width → bytes (PNG maps).
    let prefer_bits = bpp < 8 || (stride * 8 < width * 2);
    if prefer_bits && bit_solids >= 64 {
        Ok(bit_m)
    } else if byte_solids >= 64 {
        Ok(byte_m)
    } else {
        Ok(bit_m)
    }
}

fn read_grid_raw() -> Result<(Vec<u8>, usize, usize, usize, u32), &'static str> {
    let g = grid_view()?;
    let nbytes = g.stride.saturating_mul(g.height);
    let mut packed = vec![0u8; nbytes];
    unsafe {
        std::ptr::copy_nonoverlapping(g.bits as *const u8, packed.as_mut_ptr(), nbytes);
    }
    Ok((packed, g.stride, g.width, g.height, g.bpp))
}

struct GridView {
    bits: usize,
    stride: usize,
    width: usize,
    height: usize,
    bpp: u32,
}

fn grid_view() -> Result<GridView, &'static str> {
    let world = wa_mem::active_world()?;
    let land = wa_mem::read_ptr(world + OFF_WORLD_LANDSCAPE).ok_or("null Landscape*")?;
    if land == 0 || !wa_mem::ptr_readable(land, 0x920) {
        return Err("Landscape not readable");
    }
    let grid = wa_mem::read_ptr(land + OFF_LAND_SOLID_GRID).ok_or("null +0x910")?;
    if grid == 0 || !wa_mem::ptr_readable(grid, 0x2c) {
        return Err("+0x910 BitGrid bad");
    }
    let bits = wa_mem::read_ptr(grid + OFF_GRID_BITS).ok_or("null bits*")?;
    let stride = wa_mem::read_u32_at(grid + OFF_GRID_STRIDE).unwrap_or(0) as usize;
    let mut w = wa_mem::read_i32_at(grid + OFF_GRID_W).unwrap_or(0);
    let mut h = wa_mem::read_i32_at(grid + OFF_GRID_H).unwrap_or(0);
    if w <= 0 || h <= 0 {
        w = wa_mem::read_i32_at(world + OFF_WORLD_MAP_W).unwrap_or(0);
        h = wa_mem::read_i32_at(world + OFF_WORLD_MAP_H).unwrap_or(0);
    }
    if w <= 0 || h <= 0 || w > MAX_MAP_W || h > MAX_MAP_H {
        return Err("bad map size");
    }
    let bpp = wa_mem::read_u32_at(grid + OFF_GRID_BPP).unwrap_or(1);
    let height = h as usize;
    let width = w as usize;
    if bits == 0 || stride == 0 || !wa_mem::ptr_readable(bits, stride.saturating_mul(height)) {
        return Err("bits plane not readable");
    }
    Ok(GridView {
        bits,
        stride,
        width,
        height,
        bpp,
    })
}

#[inline]
fn grid_is_solid(g: &GridView, x: i32, y: i32) -> bool {
    if x < 0 || y < 0 || x >= g.width as i32 || y >= g.height as i32 {
        return false;
    }
    let ux = x as usize;
    let uy = y as usize;
    if g.bpp >= 8 {
        let row = uy * g.stride;
        wa_mem::read_u8_at(g.bits + row + ux).unwrap_or(0) != 0
    } else {
        let bi = uy * g.stride + (ux >> 3);
        let b = wa_mem::read_u8_at(g.bits + bi).unwrap_or(0);
        (b >> (ux & 7)) & 1 != 0
    }
}

/// Top solid pixel in column `x` (scan sky → ground). Non-zero = land in stock mask.
pub fn surface_top_y_at(x: f64) -> Option<f64> {
    let g = grid_view().ok()?;
    let col = x.round() as i32;
    if col < 0 || col >= g.width as i32 {
        return None;
    }
    for y in 0..g.height as i32 {
        if grid_is_solid(&g, col, y) {
            return Some(y as f64);
        }
    }
    None
}

/// Snap arc landing `(x, y)` onto the fairway surface at that column.
pub fn snap_landing_to_surface(x: f64, _y: f64) -> Option<(f64, f64)> {
    surface_top_y_at(x).map(|sy| (x, sy))
}

/// Copy Landscape+0x910. Optional worm pos picks byte vs bit via feet probe.
pub fn snapshot_solid_fill_at(worm_x: f64, worm_y: f64) -> Result<LandscapeMask, &'static str> {
    let (packed, stride, width, height, bpp) = read_grid_raw()?;
    build_mask(&packed, stride, width, height, bpp, Some((worm_x, worm_y)))
}

pub fn snapshot_solid_fill() -> Result<LandscapeMask, &'static str> {
    let (packed, stride, width, height, bpp) = read_grid_raw()?;
    build_mask(&packed, stride, width, height, bpp, None)
}

/// Reject det coords from freed/stale entity reads (e.g. wind drift off-map).
pub fn plausible_ball_pos(x: f64, y: f64) -> bool {
    let Ok(land) = snapshot_solid_fill() else {
        return x >= -64.0 && x <= 20_000.0 && y >= -64.0 && y <= 4096.0;
    };
    land.in_bounds(x, y) || (x >= -32.0 && y >= -32.0 && x <= land.width as f64 + 32.0 && y <= land.height as f64 + 32.0)
}
