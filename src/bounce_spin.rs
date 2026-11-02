//! In-flight golf balls — bounce on gravity hook; fuse freeze + idle-arm on missile FrameFinish.
//!
//! Idle-arm uses physics-frame counts on MissileEntity HandleMessage (runs while at rest).
//! Worker may also freeze as backup but must never arm (wall-clock arm desyncs).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::log;
use crate::spin::{self, SpinMode};
use crate::wa_mem;

const BOUNCE_VY_MIN: i32 = 2 << 16;
/// ~1s at 50 Hz — counted on FrameFinish for both lockstep peers.
const IDLE_ARM_TICKS: u32 = 50;

struct LiveBall {
    spin: SpinMode,
    launch_power: f64,
    bounce_count: u32,
    last_vy: Option<i32>,
    idle_ticks: u32,
    armed_rest: Option<(f64, f64)>,
}

fn live() -> &'static Mutex<HashMap<usize, LiveBall>> {
    static LIVE: OnceLock<Mutex<HashMap<usize, LiveBall>>> = OnceLock::new();
    LIVE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn floor_bounce(prev_vy: i32, vy: i32) -> bool {
    prev_vy > BOUNCE_VY_MIN && vy < 0
}

pub fn register(ptr: usize, spin: SpinMode, launch_power: f64) {
    if ptr == 0 {
        return;
    }
    let last_vy = wa_mem::read_missile_vel(ptr).map(|(_, vy)| vy);
    if let Ok(mut map) = live().lock() {
        map.insert(
            ptr,
            LiveBall {
                spin,
                launch_power,
                bounce_count: 0,
                last_vy,
                idle_ticks: 0,
                armed_rest: None,
            },
        );
    }
    let _ = wa_mem::freeze_missile_fuse(ptr);
}

pub fn unregister(ptr: usize) {
    if let Ok(mut map) = live().lock() {
        map.remove(&ptr);
    }
}

pub fn is_tracked(ptr: usize) -> bool {
    live()
        .lock()
        .map(|m| m.contains_key(&ptr))
        .unwrap_or(false)
}

pub fn armed_rest(ptr: usize) -> Option<(f64, f64)> {
    live()
        .lock()
        .ok()?
        .get(&ptr)
        .and_then(|b| b.armed_rest)
}

pub fn launch_power_of(ptr: usize) -> f64 {
    wa_mem::read_missile_vel(ptr)
        .map(|(vx, vy)| spin::power_from_missile_vel(vx, vy))
        .unwrap_or(1.0)
}

/// Missile FrameFinish — pin fuse before stock −0x14 tick.
pub fn pre_frame_missile(ptr: usize) {
    if ptr == 0 || !is_tracked(ptr) {
        return;
    }
    let armed = live()
        .lock()
        .ok()
        .and_then(|m| m.get(&ptr).map(|b| b.armed_rest.is_some()))
        .unwrap_or(false);
    if !armed {
        let _ = wa_mem::freeze_missile_fuse(ptr);
    }
}

/// Missile FrameFinish after stock — idle-arm on the same tick for both peers.
pub fn post_frame_missile(ptr: usize) {
    if ptr == 0 || !wa_mem::is_missile_ptr(ptr) {
        return;
    }
    let Ok(mut map) = live().lock() else {
        return;
    };
    let Some(ball) = map.get_mut(&ptr) else {
        return;
    };
    if ball.armed_rest.is_some() {
        return;
    }

    if wa_mem::missile_is_idle(ptr) {
        ball.idle_ticks = ball.idle_ticks.saturating_add(1);
        if ball.idle_ticks >= IDLE_ARM_TICKS {
            let (x, y) = wa_mem::read_missile_pos(ptr).unwrap_or((0.0, 0.0));
            if wa_mem::arm_missile_detonate(ptr) {
                ball.armed_rest = Some((x, y));
                log::line(&format!(
                    "[wkGolf] ball rested ({x:.1},{y:.1}) — armed stock detonate (frame-idle {})",
                    ball.idle_ticks
                ));
            }
        }
    } else {
        ball.idle_ticks = 0;
        // Re-pin after stock may have decremented during a busy frame.
        let _ = wa_mem::freeze_missile_fuse(ptr);
    }
}

/// Gravity hook — bounce kicks only (after stock collision response).
pub fn on_gravity_missile(ptr: usize) {
    if ptr == 0 || !wa_mem::is_missile_ptr(ptr) {
        return;
    }
    let Ok(mut map) = live().lock() else {
        return;
    };
    let Some(ball) = map.get_mut(&ptr) else {
        return;
    };
    if ball.armed_rest.is_some() {
        return;
    }
    let Some((_, vy)) = wa_mem::read_missile_vel(ptr) else {
        return;
    };
    if let Some(prev_vy) = ball.last_vy {
        if floor_bounce(prev_vy, vy) {
            apply_kick(ptr, ball);
        }
    }
    ball.last_vy = Some(vy);
}

fn apply_kick(ptr: usize, ball: &mut LiveBall) {
    let idx = ball.bounce_count;
    ball.bounce_count = ball.bounce_count.saturating_add(1);
    let kick = ball.spin.vx_impulse_scaled(ball.launch_power, idx);
    if kick == 0 {
        return;
    }
    if wa_mem::add_missile_vx(ptr, kick) {
        log::line(&format!(
            "[wkGolf] bounce {} spin {} power={:.0}% vx+={} (game-thread)",
            idx + 1,
            ball.spin.hud_label(),
            ball.launch_power * 100.0,
            kick >> 16
        ));
    }
}
