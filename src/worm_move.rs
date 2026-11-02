//! Reposition worm — write +0x84/+0x88; spawn in air above ground so the worm falls.

use crate::log;
use crate::terrain;
use crate::wa_mem;

/// Pixels above the walkable lip — clears worm hitbox vs thick tee pads.
const AIR_DROP_PX: f64 = 40.0;

fn f64_to_fixed(v: f64) -> i32 {
    (v * 65536.0).round() as i32
}

/// Find a standing Y at column `x`: air at feet, solid within a few px below.
pub fn surface_y_at(x: f64, hint_y: f64) -> Option<f64> {
    let land = terrain::snapshot_solid_fill().ok()?;
    let col = x.round() as i32;
    if col < 0 || col >= land.width as i32 {
        return None;
    }
    let start = (hint_y.round() as i32).clamp(0, land.height as i32 - 1);
    for dy in 0..120 {
        for y in [start - dy, start + dy] {
            if y < 0 || y >= land.height as i32 {
                continue;
            }
            if !land.is_solid(col, y) && land.is_solid(col, y + 1) {
                return Some(y as f64);
            }
        }
    }
    None
}

fn write_pos(worm: usize, x: f64, y: f64) -> bool {
    let xf = f64_to_fixed(x);
    let yf = f64_to_fixed(y);
    let xb = xf.to_le_bytes();
    let yb = yf.to_le_bytes();
    if !wa_mem::write_bytes_at(worm + 0x84, &xb) || !wa_mem::write_bytes_at(worm + 0x88, &yb) {
        return false;
    }
    true
}

fn air_y_above_column(x: f64, y_hint: f64) -> f64 {
    if let Some(lip) = surface_y_at(x, y_hint) {
        return (lip - AIR_DROP_PX).max(0.0);
    }
    (y_hint - AIR_DROP_PX).max(0.0)
}

pub fn tee_x_for_team(tee_x: f64, team: u32) -> f64 {
    tee_x + (team as f64) * 6.0
}

/// Place worm in open air at column `x`; stock gravity lands them on the surface.
pub fn place_worm_drop(worm: usize, x: f64, y_hint: f64) -> bool {
    let y = air_y_above_column(x, y_hint);
    if !write_pos(worm, x, y) {
        log::line(&format!("[wkGolf] place_worm_drop FAIL write ({x:.1},{y:.1})"));
        return false;
    }
    if let Some((rx, ry)) = wa_mem::read_worm_pos(worm) {
        log::line(&format!(
            "[wkGolf] place_worm_drop ({x:.1},{y:.1}) read=({rx:.1},{ry:.1})"
        ));
    }
    true
}
