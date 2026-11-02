//! Grenade first-bounce sim — wkAimbot / Blas parity (50 Hz, projectile atlas mask).

use crate::terrain::LandscapeMask;

pub const GRAVITY_PX_PER_TICK2: f64 = 0x3d70 as f64 / 65536.0;
const SPAWN_GRACE_TICKS: u32 = 5;
pub const BOUNCE_NORMAL: f64 = 0.30;

pub const SPRITE_HALF: f64 = 30.0;
const MASK_W: i32 = 7;
const MASK_H: i32 = 5;
const MASK_OX: f64 = 26.0;
const MASK_OY: f64 = 55.0;

#[derive(Clone, Copy, Debug)]
pub struct FirstBounce {
    pub x: f64,
    pub y: f64,
    pub is_wall: bool,
}

fn mask_hits_land(land: &LandscapeMask, cx: f64, cy: f64) -> bool {
    let ax = cx - SPRITE_HALF;
    let ay = cy - SPRITE_HALF;
    for ly in 0..MASK_H {
        for lx in 0..MASK_W {
            let sx = (ax + MASK_OX + lx as f64).round() as i32;
            let sy = (ay + MASK_OY + ly as f64).round() as i32;
            if land.is_solid(sx, sy) {
                return true;
            }
        }
    }
    false
}

fn unembed_mask(land: &LandscapeMask, x: &mut f64, y: &mut f64) {
    for _ in 0..64 {
        if !mask_hits_land(land, *x, *y) {
            return;
        }
        *y -= 1.0;
    }
}

/// Stop at the first terrain contact (floor or wall). Wind is not applied.
pub fn simulate_first_bounce(
    land: &LandscapeMask,
    x0: f64,
    y0: f64,
    vx0: f64,
    vy0: f64,
    fuse_ticks: u32,
    gravity_px_tick2: f64,
) -> Option<FirstBounce> {
    let mut x = x0;
    let mut y = y0;
    let vel_x = vx0;
    let mut vel_y = vy0;
    let mut ticks_left = fuse_ticks;
    let mut tick_idx = 0u32;

    if mask_hits_land(land, x, y) {
        unembed_mask(land, &mut x, &mut y);
    }

    while ticks_left > 0 {
        ticks_left -= 1;
        tick_idx += 1;
        vel_y += gravity_px_tick2;
        let dx = vel_x;
        let dy = vel_y;
        let steps = ((dx.abs().max(dy.abs())).ceil() as i32).max(1) as usize;
        let step_x = dx / steps as f64;
        let step_y = dy / steps as f64;
        let allow_bounce = tick_idx > SPAWN_GRACE_TICKS;

        for _ in 0..steps {
            if step_x.abs() > 1e-4 {
                let nx = x + step_x;
                if mask_hits_land(land, nx, y) {
                    if allow_bounce {
                        x = nx - step_x.signum();
                        unembed_mask(land, &mut x, &mut y);
                        return Some(FirstBounce {
                            x,
                            y,
                            is_wall: true,
                        });
                    }
                    unembed_mask(land, &mut x, &mut y);
                } else {
                    x = nx;
                }
            }
            if step_y.abs() > 1e-4 {
                let ny = y + step_y;
                if mask_hits_land(land, x, ny) {
                    if allow_bounce {
                        y = ny - step_y.signum();
                        unembed_mask(land, &mut x, &mut y);
                        return Some(FirstBounce {
                            x,
                            y,
                            is_wall: false,
                        });
                    }
                    unembed_mask(land, &mut x, &mut y);
                } else {
                    y = ny;
                }
            }
        }
        if !land.in_bounds(x, y) || y >= land.height as f64 + 40.0 {
            return None;
        }
    }
    None
}
