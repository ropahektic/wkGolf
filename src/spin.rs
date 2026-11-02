//! Golf-ball side spin — horizontal kick on floor bounces, scaled by shot power and bounce #.
//!
//! Online-safe dial: Enter (Jump) steps right, Backspace (JumpUp/backflip) steps left.
//! Both peers apply the same steps from lockstep WormEntity messages.

use std::sync::atomic::{AtomicU8, Ordering};

use crate::log;

/// Stock grenade release speed at 100% power (px/tick) — wkAimbot solver parity.
pub const FULL_THROW_SPEED_PX_TICK: f64 = 24.0;

pub const MAX_SPIN_TEAMS: usize = 8;

/// Dial (left ← → right), always starts at [`SpinMode::None`] each turn.
///
/// `LEFT+++ … LEFT · NO SPIN · RIGHT … RIGHT+++`
const DIAL: [SpinMode; 9] = [
    SpinMode::Left4,
    SpinMode::Left3,
    SpinMode::Left2,
    SpinMode::Left1,
    SpinMode::None,
    SpinMode::Right1,
    SpinMode::Right2,
    SpinMode::Right3,
    SpinMode::Right4,
];
const DIAL_CENTER: u8 = 4;

static SPIN_IDX: [AtomicU8; MAX_SPIN_TEAMS] = [
    AtomicU8::new(DIAL_CENTER),
    AtomicU8::new(DIAL_CENTER),
    AtomicU8::new(DIAL_CENTER),
    AtomicU8::new(DIAL_CENTER),
    AtomicU8::new(DIAL_CENTER),
    AtomicU8::new(DIAL_CENTER),
    AtomicU8::new(DIAL_CENTER),
    AtomicU8::new(DIAL_CENTER),
];

/// Side spin for the next shot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpinMode {
    #[default]
    None,
    Left1,
    Left2,
    Left3,
    Left4,
    Right1,
    Right2,
    Right3,
    Right4,
}

impl SpinMode {
    pub fn hud_label(self) -> &'static str {
        match self {
            Self::None => "NO SPIN",
            Self::Left1 => "LEFT SPIN",
            Self::Left2 => "LEFT SPIN+",
            Self::Left3 => "LEFT SPIN++",
            Self::Left4 => "LEFT SPIN+++",
            Self::Right1 => "RIGHT SPIN",
            Self::Right2 => "RIGHT SPIN+",
            Self::Right3 => "RIGHT SPIN++",
            Self::Right4 => "RIGHT SPIN+++",
        }
    }

    /// Base tier impulse at 100% power, 1st bounce (16.16 px/tick).
    /// ++ tier matches the original single-strength kick (5 px/tick).
    pub fn base_vx_impulse(self) -> i32 {
        const KICK_1: i32 = 2 << 16;
        const KICK_2: i32 = 3 << 16;
        const KICK_3: i32 = 5 << 16;
        const KICK_4: i32 = 8 << 16;
        match self {
            Self::None => 0,
            Self::Left1 => -KICK_1,
            Self::Left2 => -KICK_2,
            Self::Left3 => -KICK_3,
            Self::Left4 => -KICK_4,
            Self::Right1 => KICK_1,
            Self::Right2 => KICK_2,
            Self::Right3 => KICK_3,
            Self::Right4 => KICK_4,
        }
    }

    /// `bounce_idx`: 0 = 1st bounce (100%), 1 = 2nd (50%), 2 = 3rd (25%), 3+ = none.
    /// `power_frac`: launch power 0..1 from shot speed vs full throw.
    pub fn vx_impulse_scaled(self, power_frac: f64, bounce_idx: u32) -> i32 {
        let base = self.base_vx_impulse();
        if base == 0 {
            return 0;
        }
        let bounce_scale = match bounce_idx {
            0 => 1.0,
            1 => 0.5,
            2 => 0.25,
            _ => 0.0,
        };
        if bounce_scale <= 0.0 {
            return 0;
        }
        let power = power_frac.clamp(0.0, 1.0);
        if power <= 0.0 {
            return 0;
        }
        ((base as f64) * power * bounce_scale).round() as i32
    }
}

fn idx_for(team: u32) -> Option<usize> {
    let ti = team as usize;
    (ti < MAX_SPIN_TEAMS).then_some(ti)
}

fn mode_at(idx: u8) -> SpinMode {
    DIAL[usize::from(idx).min(DIAL.len() - 1)]
}

pub fn get(team: u32) -> SpinMode {
    let Some(ti) = idx_for(team) else {
        return SpinMode::None;
    };
    mode_at(SPIN_IDX[ti].load(Ordering::SeqCst))
}

/// Force dial to NO SPIN (every turn handoff / StartTurn). Always logs.
pub fn reset_turn(team: u32, reason: &str) {
    let Some(ti) = idx_for(team) else {
        return;
    };
    SPIN_IDX[ti].store(DIAL_CENTER, Ordering::SeqCst);
    log::line(&format!(
        "[wkGolf] T{team} spin → NO SPIN ({reason})"
    ));
}

/// Backspace / JumpUp — step toward LEFT+++ (clamp).
pub fn step_left(team: u32) -> SpinMode {
    step(team, -1, "Backspace")
}

/// Enter / Jump — step toward RIGHT+++ (clamp).
pub fn step_right(team: u32) -> SpinMode {
    step(team, 1, "Enter")
}

fn step(team: u32, delta: i8, key: &str) -> SpinMode {
    let Some(ti) = idx_for(team) else {
        return SpinMode::None;
    };
    let cur = SPIN_IDX[ti].load(Ordering::SeqCst) as i8;
    let next = (cur + delta).clamp(0, (DIAL.len() - 1) as i8) as u8;
    SPIN_IDX[ti].store(next, Ordering::SeqCst);
    let mode = mode_at(next);
    log::line(&format!(
        "[wkGolf] T{team} spin → {} ({key})",
        mode.hud_label()
    ));
    mode
}

/// Infer normalized launch power from initial missile speed.
pub fn power_from_missile_vel(vx: i32, vy: i32) -> f64 {
    let vx = vx as f64 / 65536.0;
    let vy = vy as f64 / 65536.0;
    (vx.hypot(vy) / FULL_THROW_SPEED_PX_TICK).clamp(0.0, 1.0)
}
