//! Track one grenade MissileEntity — worker observes + keeps fuse frozen in flight.
//! Idle-arm + bounce kicks stay on the game thread (`bounce_spin`) for lockstep.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use crate::bounce_spin;
use crate::log;
use crate::spin::SpinMode;
use crate::wa_mem;

const TRACK_TIMEOUT_SECS: u64 = 45;
/// After game-thread arm, wait this long for stock free before forcing teleport.
const ARMED_TIMEOUT_MS: u64 = 2500;

pub fn missile_baseline() -> HashSet<usize> {
    wa_mem::list_missiles().into_iter().map(|m| m.ptr).collect()
}

/// How a tracked grenade ended — only `Rested` commits a new ball position.
#[derive(Clone, Copy, Debug)]
pub enum DetOutcome {
    /// Idle on terrain → stock detonate (normal golf stop).
    Rested { x: f64, y: f64 },
    /// Entity removed before rest/detonate (drown, OOB sink, stale ptr).
    Lost { label: &'static str },
}

pub struct DetTracker {
    ptr: usize,
    last_x: f64,
    last_y: f64,
    saw_armed: bool,
    armed_at: Option<Instant>,
    track_until: Instant,
}

impl DetTracker {
    pub fn ptr(&self) -> usize {
        self.ptr
    }

    pub fn last_pos(&self) -> (f64, f64) {
        (self.last_x, self.last_y)
    }

    pub fn new(ptr: usize, x: f64, y: f64, spin: SpinMode) -> Self {
        let _ = wa_mem::freeze_missile_fuse(ptr);
        let launch_power = bounce_spin::launch_power_of(ptr);
        bounce_spin::register(ptr, spin, launch_power);
        log::line(&format!(
            "[wkGolf] grenade track ptr=0x{ptr:08X} first=({x:.1},{y:.1}) spin={} power={:.0}% fuse=off",
            spin.hud_label(),
            launch_power * 100.0
        ));
        Self {
            ptr,
            last_x: x,
            last_y: y,
            saw_armed: false,
            armed_at: None,
            track_until: Instant::now() + Duration::from_secs(TRACK_TIMEOUT_SECS),
        }
    }

    fn rest_coords(&self) -> (f64, f64) {
        bounce_spin::armed_rest(self.ptr).unwrap_or((self.last_x, self.last_y))
    }

    fn finish(&mut self, outcome: DetOutcome) -> DetOutcome {
        bounce_spin::unregister(self.ptr);
        outcome
    }

    pub fn poll(&mut self) -> Option<DetOutcome> {
        if Instant::now() > self.track_until {
            let (x, y) = self.rest_coords();
            log::line(&format!(
                "[wkGolf] ball lost @ ({x:.1},{y:.1}) (track timeout)"
            ));
            return Some(self.finish(DetOutcome::Lost {
                label: "track timeout",
            }));
        }

        if let Some((rx, ry)) = bounce_spin::armed_rest(self.ptr) {
            if !self.saw_armed {
                self.saw_armed = true;
                self.armed_at = Some(Instant::now());
                self.last_x = rx;
                self.last_y = ry;
                log::line(&format!(
                    "[wkGolf] worker saw rest-arm @ ({rx:.1},{ry:.1}) — waiting stock free"
                ));
            }
        }

        if self.saw_armed {
            if let Some(at) = self.armed_at {
                if at.elapsed() >= Duration::from_millis(ARMED_TIMEOUT_MS) {
                    let (x, y) = self.rest_coords();
                    log::line(&format!(
                        "[wkGolf] detonation ≈ ({x:.1},{y:.1}) (armed timeout — rest snapshot)"
                    ));
                    return Some(self.finish(DetOutcome::Rested { x, y }));
                }
            }
            // Do not follow drifted pos; do not freeze (fuse must stay 0).
            if wa_mem::read_missile_pos(self.ptr).is_some() {
                return None;
            }
            let (x, y) = self.rest_coords();
            log::line(&format!(
                "[wkGolf] detonation ≈ ({x:.1},{y:.1}) (stock)"
            ));
            return Some(self.finish(DetOutcome::Rested { x, y }));
        }

        let Some((x, y)) = wa_mem::read_missile_pos(self.ptr) else {
            log::line(&format!(
                "[wkGolf] ball lost @ ({:.1},{:.1}) (entity gone — no rest detonate)",
                self.last_x, self.last_y
            ));
            return Some(self.finish(DetOutcome::Lost {
                label: "entity gone early",
            }));
        };

        self.last_x = x;
        self.last_y = y;
        // Keep fuse pinned while flying — gravity-hook alone misses other fuse ticks.
        let _ = wa_mem::freeze_missile_fuse_unless_detonating(self.ptr);
        None
    }
}

impl Drop for DetTracker {
    fn drop(&mut self) {
        bounce_spin::unregister(self.ptr);
    }
}
