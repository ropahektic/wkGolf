//! Max-power landing guide — stock release_focus (+0x24c) vacuum arc.

use std::f64::consts::TAU;

use crate::ballistics::{self, GRAVITY_PX_PER_TICK2};
use crate::spin::FULL_THROW_SPEED_PX_TICK;

const SPAWN_GRACE_TICKS: u32 = 5;

/// WeaponRelease spawn offset along aim (px).
pub const SPAWN_OFFSET_PX: f64 = 24.0;
/// Flat fairway through worm feet — no landscape read.
const FEET_TO_GROUND_PX: f64 = 36.0;
const MAX_VACUUM_TICKS: u32 = 12_000;

const FIXED_ONE: u32 = 0x10000;

/// RE WormEntity__release_focus / wkAimbot tele::gun_release_unit.
/// Input is worm weapon aim (+0x24c) — ≈ 2× shared crosshair elevation; SAR 1 before fixsin.
pub fn gun_release_unit(weapon_aim: u32, facing: i32) -> (f64, f64) {
    let mut angle = weapon_aim & 0xffff;
    if facing < 0 {
        angle = 0x20000u32.wrapping_sub(angle);
    }
    angle >>= 1;
    let theta = (angle as f64) * TAU / (FIXED_ONE as f64);
    (theta.sin(), theta.cos())
}

/// Fallback when +0x24c unreadable: stock stores ~2× crosshair display in weapon aim.
pub fn grenade_release_from_display(display: u32, facing: i32) -> (f64, f64) {
    let weapon_aim = ((display & 0xffff) as u32).saturating_mul(2);
    gun_release_unit(weapon_aim, facing)
}

/// Stock 50 Hz vacuum — integrate until flat ground (fuse-less golf grenade).
pub fn vacuum_ground_impact(
    x0: f64,
    y0: f64,
    vx: f64,
    vy: f64,
    ground_y: f64,
    gravity_px_tick2: f64,
) -> Option<(f64, f64)> {
    let mut x = x0;
    let mut y = y0;
    let vel_x = vx;
    let mut vel_y = vy;
    for t in 1..=MAX_VACUUM_TICKS {
        vel_y += gravity_px_tick2;
        x += vel_x;
        y += vel_y;
        if t > SPAWN_GRACE_TICKS && y >= ground_y {
            return Some((x, y));
        }
    }
    None
}

pub fn gravity_px_tick2(low_gravity: bool) -> f64 {
    GRAVITY_PX_PER_TICK2
        * if low_gravity {
            crate::wa_mem::WA_LOW_GRAVITY_SCALE
        } else {
            1.0
        }
}

/// 100% power landing on flat ground from weapon aim (+0x24c).
pub fn max_power_vacuum_ground_hit(
    worm_x: f64,
    worm_y: f64,
    weapon_aim: u32,
    facing: i32,
    low_gravity: bool,
) -> Option<(f64, f64)> {
    let (ux, uy) = gun_release_unit(weapon_aim, facing);
    let sx = worm_x + ux * SPAWN_OFFSET_PX;
    let sy = worm_y + uy * SPAWN_OFFSET_PX;
    let speed = FULL_THROW_SPEED_PX_TICK;
    let ground_y = worm_y + FEET_TO_GROUND_PX;
    vacuum_ground_impact(
        sx,
        sy,
        ux * speed,
        uy * speed,
        ground_y,
        gravity_px_tick2(low_gravity),
    )
}

pub fn vacuum_ground_impact_from_vel(
    x0: f64,
    y0: f64,
    vx_fix: i32,
    vy_fix: i32,
    ground_y: f64,
    low_gravity: bool,
) -> Option<(f64, f64)> {
    vacuum_ground_impact(
        x0,
        y0,
        vx_fix as f64 / 65536.0,
        vy_fix as f64 / 65536.0,
        ground_y,
        gravity_px_tick2(low_gravity),
    )
}

#[allow(dead_code)]
pub const SIM_FUSE_TICKS: u32 = 6000;

#[allow(dead_code)]
pub fn max_power_first_bounce_on_land(
    land: &crate::terrain::LandscapeMask,
    worm_x: f64,
    worm_y: f64,
    weapon_aim: u32,
    facing: i32,
) -> Option<(f64, f64)> {
    let (ux, uy) = gun_release_unit(weapon_aim, facing);
    let sx = worm_x + ux * SPAWN_OFFSET_PX;
    let sy = worm_y + uy * SPAWN_OFFSET_PX;
    let speed = FULL_THROW_SPEED_PX_TICK;
    let hit = ballistics::simulate_first_bounce(
        land,
        sx,
        sy,
        ux * speed,
        uy * speed,
        SIM_FUSE_TICKS,
        GRAVITY_PX_PER_TICK2,
    )?;
    Some((hit.x, hit.y))
}
