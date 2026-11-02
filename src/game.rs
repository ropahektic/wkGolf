//! Mini-golf: grenade = ball; teleport shooter to det / tee via place_worm_drop.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use crate::config::Course;
use crate::draw;
use crate::log;
use crate::aim;
use crate::missile::{self, DetOutcome, DetTracker};
use crate::spin::{self, SpinMode};
use crate::terrain;
use crate::wa_mem;
use crate::worm_move;

const MAX_TEAMS: u32 = 8;
const TURN_SETTLE_MS: u64 = 800;

struct PendingShot {
    team: u32,
    worm_idx: u32,
    stroke_n: u32,
    tracker: DetTracker,
}

pub struct GolfGame {
    course: Course,
    /// Per-team index into `course.holes`.
    team_hole: [usize; MAX_TEAMS as usize],
    strokes: [u32; MAX_TEAMS as usize],
    totals: [u32; MAX_TEAMS as usize],
    course_done: [bool; MAX_TEAMS as usize],
    ball_x: [Option<f64>; MAX_TEAMS as usize],
    ball_y: [Option<f64>; MAX_TEAMS as usize],
    /// Hole index already advanced — worm still needs drop on that hole's tee.
    pending_tee_snap: [bool; MAX_TEAMS as usize],
    pending_dets: [Option<PendingShot>; MAX_TEAMS as usize],
    watch_turn: Option<(u32, u32)>,
    /// Per-team settle gate — finishing T1 must not block T2's next shot.
    turn_ready_at: [Option<Instant>; MAX_TEAMS as usize],
    /// Per-team missile baseline (stale ptr reuse safe).
    missile_base_snapshotted: [bool; MAX_TEAMS as usize],
    missile_base: [HashSet<usize>; MAX_TEAMS as usize],
    last_hud: Instant,
    session_started: bool,
    teams_seen: u32,
    center_msg: Option<String>,
    center_until: Option<Instant>,
    /// Last HUD spin label per team — refresh when lockstep dial changes.
    spin_hud: [SpinMode; MAX_TEAMS as usize],
}

impl GolfGame {
    pub fn new(course: Course) -> Self {
        Self {
            course,
            team_hole: [0; MAX_TEAMS as usize],
            strokes: [0; MAX_TEAMS as usize],
            totals: [0; MAX_TEAMS as usize],
            course_done: [false; MAX_TEAMS as usize],
            ball_x: [None; MAX_TEAMS as usize],
            ball_y: [None; MAX_TEAMS as usize],
            pending_tee_snap: [false; MAX_TEAMS as usize],
            pending_dets: std::array::from_fn(|_| None),
            watch_turn: None,
            turn_ready_at: [None; MAX_TEAMS as usize],
            missile_base_snapshotted: [false; MAX_TEAMS as usize],
            missile_base: std::array::from_fn(|_| HashSet::new()),
            last_hud: Instant::now() - Duration::from_secs(1),
            session_started: false,
            teams_seen: 0,
            center_msg: None,
            center_until: None,
            spin_hud: [SpinMode::None; MAX_TEAMS as usize],
        }
    }

    fn set_center_announcement(&mut self, msg: String) {
        self.center_msg = Some(msg);
        self.center_until = Some(Instant::now() + Duration::from_secs(5));
    }

    fn active_center_msg(&mut self) -> String {
        if let Some(until) = self.center_until {
            if Instant::now() >= until {
                self.center_msg = None;
                self.center_until = None;
            }
        }
        self.center_msg.clone().unwrap_or_default()
    }

    fn ordinal_shot(n: u32) -> String {
        let ord = match n {
            1 => "1ST",
            2 => "2ND",
            3 => "3RD",
            4 => "4TH",
            5 => "5TH",
            6 => "6TH",
            7 => "7TH",
            8 => "8TH",
            9 => "9TH",
            n => return format!("{n}TH SHOT"),
        };
        format!("{ord} SHOT")
    }

    fn play_status(strokes: u32, par: u32) -> String {
        if strokes == 0 {
            return format!("FOR PAR {par}");
        }
        match (par as i32) - (strokes as i32) {
            n if n > 0 => format!("{n} TO PAR"),
            0 => "AT PAR".into(),
            n => format!("{} OVER", -n),
        }
    }

    fn active_turn_worm(&self) -> Option<(u32, u32)> {
        if let Ok((_, _, team, worm_idx)) = wa_mem::current_turn() {
            return Some((team, worm_idx));
        }
        if let Ok(ids) = wa_mem::current_turn_ids() {
            return Some(ids);
        }
        self.watch_turn
    }

    fn hole_result(strokes: u32, par: u32) -> String {
        if strokes == 1 {
            return "HOLE IN ONE!".into();
        }
        let diff = strokes as i32 - par as i32;
        match diff {
            -4..=-3 => "ALBATROSS!".into(),
            -2 => "EAGLE!".into(),
            -1 => "BIRDIE!".into(),
            0 => "PAR!".into(),
            1 => "BOGEY!".into(),
            2 => "DOUBLE BOGEY!".into(),
            3 => "TRIPLE BOGEY!".into(),
            n if n > 3 => format!("+{n} OVER PAR"),
            _ => "GREAT SHOT!".into(),
        }
    }

    fn worm_name_for_turn(team: u32, worm_idx: u32) -> String {
        if let Ok(worms) = wa_mem::list_alive_worms() {
            for w in worms {
                if w.team == team && w.worm_idx == worm_idx {
                    return w.name;
                }
            }
        }
        format!("Team {}", team + 1)
    }

    pub fn tick(&mut self) {
        if !wa_mem::in_game_session() {
            if self.session_started {
                self.log_session_end();
                draw::clear_hud();
                *self = GolfGame::new(self.course.clone());
            }
            return;
        }
        self.session_started = true;

        if self.teams_seen == 0 {
            self.refresh_teams_seen();
            if self.teams_seen > 0 {
                self.spawn_all_at_tee();
            }
        }

        self.poll_pending_det();
        self.retry_pending_tee_snaps();
        wa_mem::maintain_golf_missiles();

        let Ok((worm, _world, team, worm_idx)) = wa_mem::current_turn() else {
            return;
        };

        self.sync_spin_hud(team);
        self.update_landing_guide(team, worm);
        draw::sync_landing_mark(team);
        self.note_turn_handoff(team, worm_idx, worm);

        if !self.course_done[team as usize] {
            self.ensure_missile_base_snapshot(team);
            self.watch_for_grenade_spawn(worm, team, worm_idx);
        }

        if self.last_hud.elapsed() >= Duration::from_millis(250) {
            self.draw_hud();
            self.last_hud = Instant::now();
        }
    }

    fn sync_spin_hud(&mut self, team: u32) {
        let ti = team as usize;
        if team >= MAX_TEAMS || self.course_done[ti] {
            return;
        }
        let cur = spin::get(team);
        if cur != self.spin_hud[ti] {
            self.spin_hud[ti] = cur;
            self.draw_hud();
            self.last_hud = Instant::now();
        }
    }

    fn update_landing_guide(&mut self, team: u32, worm: usize) {
        let ti = team as usize;
        if team >= MAX_TEAMS
            || self.course_done[ti]
            || self.pending_dets[ti].is_some()
        {
            draw::set_landing_mark(None);
            return;
        }
        let weapon_aim = wa_mem::read_worm_aim(worm)
            .or_else(|| {
                wa_mem::read_worm_aim_smooth(worm)
                    .map(|d| (d & 0xffff).saturating_mul(2))
            })
            .unwrap_or(0);
        let facing = wa_mem::read_worm_facing(worm).unwrap_or(-1);
        let low_gravity = wa_mem::read_worm_low_gravity(worm);
        let (wx, wy) = wa_mem::read_worm_pos(worm).unwrap_or((0.0, 0.0));
        let mark = aim::max_power_vacuum_ground_hit(wx, wy, weapon_aim, facing, low_gravity)
            .and_then(|(lx, ly)| terrain::snap_landing_to_surface(lx, ly));
        if let Some((lx, ly)) = mark {
            static mut ARC_LOG: u32 = 0;
            if unsafe { ARC_LOG } < 4 {
                unsafe { ARC_LOG += 1; }
                let (ux, uy) = aim::gun_release_unit(weapon_aim, facing);
                log::line(&format!(
                    "[wkGolf] arc aim=0x{weapon_aim:04X} f={facing} lg={low_gravity} u=({ux:.2},{uy:.2}) land=({lx:.0},{ly:.0}) worm=({wx:.0},{wy:.0})"
                ));
            }
        }
        draw::set_landing_mark(mark);
    }

    fn note_turn_handoff(&mut self, team: u32, worm_idx: u32, worm: usize) {
        let ids = (team, worm_idx);
        let ti = team as usize;
        let turn_changed = self.watch_turn != Some(ids);
        let needs_tee = ti < MAX_TEAMS as usize && self.pending_tee_snap[ti];

        if !turn_changed && !needs_tee {
            return;
        }

        if turn_changed {
            self.watch_turn = Some(ids);
            if ti < MAX_TEAMS as usize {
                spin::reset_turn(team, "handoff");
                self.spin_hud[ti] = SpinMode::None;
            }
            self.begin_turn_settle(team, worm_idx, ti, "handoff");
        } else if needs_tee {
            self.begin_turn_settle(team, worm_idx, ti, "tee-snap");
        }

        if needs_tee {
            self.try_tee_snap_for_team(team, Some(worm));
        }
    }

    fn turn_ready(&self, team: u32) -> bool {
        let ti = team as usize;
        if ti >= MAX_TEAMS as usize {
            return false;
        }
        self.turn_ready_at[ti]
            .map(|t| Instant::now() >= t)
            .unwrap_or(false)
    }

    fn begin_turn_settle(&mut self, team: u32, worm_idx: u32, ti: usize, reason: &str) {
        self.missile_base_snapshotted[ti] = false;
        self.turn_ready_at[ti] = Some(Instant::now() + Duration::from_millis(TURN_SETTLE_MS));
        log::line(&format!(
            "[wkGolf] {reason} T{team}w{worm_idx} hole {} — settle {}ms",
            self.team_hole[ti] + 1,
            TURN_SETTLE_MS
        ));
    }

    fn ensure_missile_base_snapshot(&mut self, team: u32) {
        let ti = team as usize;
        if ti >= MAX_TEAMS as usize || self.missile_base_snapshotted[ti] || !self.turn_ready(team)
        {
            return;
        }
        self.missile_base[ti] = missile::missile_baseline();
        self.missile_base_snapshotted[ti] = true;
        log::line(&format!(
            "[wkGolf] T{team} missile baseline snap ({} known)",
            self.missile_base[ti].len()
        ));
    }

    fn missile_counts_as_new_shot(ptr: usize, in_baseline: bool) -> bool {
        if !in_baseline {
            return true;
        }
        // WA reuses MissileEntity slots — a baseline ptr that starts moving is a fresh shot.
        !wa_mem::missile_is_idle(ptr)
    }

    fn resolve_worm_ptr(team: u32, worm_idx: u32) -> Option<usize> {
        wa_mem::list_alive_worms()
            .ok()?
            .into_iter()
            .find(|w| w.team == team && w.worm_idx == worm_idx)
            .map(|w| w.ptr)
    }

    fn teleport_worm(team: u32, worm_idx: u32, x: f64, y: f64, label: &str) -> bool {
        let Some(ptr) = Self::resolve_worm_ptr(team, worm_idx) else {
            log::line(&format!(
                "[wkGolf] teleport FAIL T{team}w{worm_idx} {label} — worm not found"
            ));
            return false;
        };
        log::line(&format!(
            "[wkGolf] teleport T{team}w{worm_idx} {label}→({x:.0},{y:.0})"
        ));
        worm_move::place_worm_drop(ptr, x, y)
    }

    fn watch_for_grenade_spawn(&mut self, worm: usize, team: u32, worm_idx: u32) {
        let ti = team as usize;
        if team >= MAX_TEAMS || self.course_done[ti] || !self.turn_ready(team) {
            return;
        }
        if !self.missile_base_snapshotted[ti] {
            return;
        }
        let Ok(sample) = wa_mem::sample_aim_cursor() else {
            return;
        };
        if sample.turn_active == 0 {
            return;
        }
        let (wx, wy) = wa_mem::read_worm_pos(worm).unwrap_or((sample.worm_x, sample.worm_y));

        let tracked_ptr = self.pending_dets[ti].as_ref().map(|p| p.tracker.ptr());

        let mut best: Option<(usize, f64, f64, f64)> = None;
        for m in wa_mem::list_missiles() {
            if tracked_ptr == Some(m.ptr) {
                continue;
            }
            let in_base = self.missile_base[ti].contains(&m.ptr);
            if !Self::missile_counts_as_new_shot(m.ptr, in_base) {
                continue;
            }
            let d = hypot(m.x - wx, m.y - wy);
            if d >= 160.0 {
                continue;
            }
            if best.map(|b| d < b.3).unwrap_or(true) {
                best = Some((m.ptr, m.x, m.y, d));
            }
        }

        let Some((ptr, mx, my, d)) = best else {
            return;
        };

        // Stuck prior track would block marker + teleport forever — drop it for the new ball.
        if let Some(old) = self.pending_dets[ti].take() {
            let (lx, ly) = old.tracker.last_pos();
            log::line(&format!(
                "[wkGolf] T{team} abandon stale track ptr=0x{:08X} @ ({lx:.0},{ly:.0}) — new shot",
                old.tracker.ptr()
            ));
            if ti < MAX_TEAMS as usize && self.strokes[ti] > 0 {
                self.strokes[ti] -= 1;
            }
        }

        self.strokes[ti] += 1;
        let n = self.strokes[ti];
        if let Some((vx, vy)) = wa_mem::read_missile_vel(ptr) {
            let (_wx, wy) = wa_mem::read_worm_pos(worm).unwrap_or((mx, my));
            let ground_y = wy + 36.0;
            if let Some(hit) = aim::vacuum_ground_impact_from_vel(
                mx,
                my,
                vx,
                vy,
                ground_y,
                wa_mem::read_worm_low_gravity(worm),
            ) {
                log::line(&format!(
                    "[wkGolf] shot vacuum ground ({:.0},{:.0}) vel=({:.1},{:.1})",
                    hit.0,
                    hit.1,
                    vx as f64 / 65536.0,
                    vy as f64 / 65536.0
                ));
            }
        }
        log::line(&format!(
            "[wkGolf] T{team} stroke {n} hole {} (missile d={d:.0})",
            self.team_hole[ti] + 1
        ));
        self.pending_dets[ti] = Some(PendingShot {
            team,
            worm_idx,
            stroke_n: n,
            tracker: DetTracker::new(ptr, mx, my, spin::get(team)),
        });
    }

    fn log_session_end(&self) {
        let alive = wa_mem::list_alive_worms()
            .map(|w| {
                w.iter()
                    .map(|x| format!("T{}w{}", x.team, x.worm_idx))
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_else(|_| "?".into());
        let flags = wa_mem::game_session_end_flags()
            .map(|(e, s)| format!("exit={e} status={s}"))
            .unwrap_or_else(|| "flags=?".into());
        let map = wa_mem::format_turn_map();
        log::line(&format!(
            "[wkGolf] session ended — {flags} alive=[{alive}] {map}"
        ));
    }

    fn hole_for_team(&self, team: u32) -> &crate::config::Hole {
        &self.course.holes[self.team_hole[team as usize]]
    }

    fn spawn_all_at_tee(&mut self) {
        let worms = wa_mem::list_alive_worms().unwrap_or_default();
        if worms.is_empty() {
            return;
        }
        let mut placed = Vec::new();
        for w in &worms {
            if w.team >= MAX_TEAMS {
                continue;
            }
            let h = self.hole_for_team(w.team);
            let (x, y) = h.tee_world(w.team);
            if worm_move::place_worm_drop(w.ptr, x, y) {
                placed.push(format!("T{}w{}", w.team, w.worm_idx));
            }
        }
        if !placed.is_empty() {
            log::line(&format!(
                "[wkGolf] tee spawn all — {} (hole 1)",
                placed.join(",")
            ));
        }
    }

    fn refresh_teams_seen(&mut self) {
        let worms = wa_mem::list_alive_worms().unwrap_or_default();
        for w in &worms {
            if w.team < MAX_TEAMS {
                self.teams_seen = self.teams_seen.max(w.team + 1);
            }
        }
        if self.teams_seen == 0 {
            if let Ok(ids) = wa_mem::current_turn_ids() {
                self.teams_seen = ids.0 + 1;
            }
        }
    }

    fn ball_pos_for_team(&self, team: u32) -> (f64, f64) {
        let ti = team as usize;
        if ti < MAX_TEAMS as usize {
            if let (Some(x), Some(y)) = (self.ball_x[ti], self.ball_y[ti]) {
                return (x, y);
            }
        }
        self.hole_for_team(team).tee_world(team)
    }

    fn advance_team_hole(&mut self, ti: usize, team: u32) {
        if self.team_hole[ti] + 1 < self.course.holes.len() {
            self.team_hole[ti] += 1;
            self.strokes[ti] = 0;
            self.ball_x[ti] = None;
            self.ball_y[ti] = None;
            log::line(&format!(
                "[wkGolf] T{team} advancing to hole {}",
                self.team_hole[ti] + 1
            ));
        } else {
            self.course_done[ti] = true;
            self.pending_tee_snap[ti] = false;
            log::line(&format!("[wkGolf] T{team} finished all holes"));
        }
    }

    fn try_tee_snap_for_team(&mut self, team: u32, prefer_worm: Option<usize>) -> bool {
        let ti = team as usize;
        if team >= MAX_TEAMS || self.course_done[ti] || !self.pending_tee_snap[ti] {
            return false;
        }
        let worms = wa_mem::list_alive_worms().unwrap_or_default();
        let on_team: Vec<_> = worms.iter().filter(|w| w.team == team).collect();
        if on_team.is_empty() {
            log::line(&format!("[wkGolf] tee-snap FAIL T{team}: no alive worms"));
            return false;
        }
        let pick = prefer_worm
            .and_then(|pw| on_team.iter().find(|w| w.ptr == pw).copied())
            .or_else(|| on_team.first().copied());
        let Some(w) = pick else {
            log::line(&format!("[wkGolf] tee-snap FAIL T{team}: worm pick miss"));
            return false;
        };
        let h = self.hole_for_team(team);
        let (tx, ty) = h.tee_world(team);
        log::line(&format!(
            "[wkGolf] tee-snap T{team}w{} hole {} → ({tx:.0},{ty:.0})",
            w.worm_idx,
            self.team_hole[ti] + 1
        ));
        if !worm_move::place_worm_drop(w.ptr, tx, ty) {
            log::line(&format!("[wkGolf] tee-snap FAIL T{team}w{}: place_worm_drop", w.worm_idx));
            return false;
        }
        self.pending_tee_snap[ti] = false;
        self.missile_base_snapshotted[ti] = false;
        self.turn_ready_at[ti] = Some(Instant::now() + Duration::from_millis(TURN_SETTLE_MS));
        true
    }

    fn retry_pending_tee_snaps(&mut self) {
        let prefer = wa_mem::current_turn().ok().map(|(w, _, t, _)| (t, w));
        for ti in 0..MAX_TEAMS as usize {
            if !self.pending_tee_snap[ti] {
                continue;
            }
            let team = ti as u32;
            let worm = prefer
                .filter(|(t, _)| *t as usize == ti)
                .map(|(_, w)| w);
            self.try_tee_snap_for_team(team, worm);
        }
    }

    fn after_shot_teleport(&mut self, ti: usize) {
        if ti >= MAX_TEAMS as usize {
            return;
        }
        self.missile_base_snapshotted[ti] = false;
        self.turn_ready_at[ti] = Some(Instant::now() + Duration::from_millis(TURN_SETTLE_MS));
    }

    fn poll_pending_det(&mut self) {
        for ti in 0..MAX_TEAMS as usize {
            let Some(mut pending) = self.pending_dets[ti].take() else {
                continue;
            };
            let Some(outcome) = pending.tracker.poll() else {
                self.pending_dets[ti] = Some(pending);
                continue;
            };
            self.finish_pending_shot(pending, outcome);
        }
    }

    fn finish_pending_shot(&mut self, pending: PendingShot, outcome: DetOutcome) {
        let team = pending.team;
        let worm_idx = pending.worm_idx;
        let n = pending.stroke_n;
        let ti = team as usize;

        let (dx, dy) = match outcome {
            DetOutcome::Rested { x, y } => (x, y),
            DetOutcome::Lost { label } => {
                if ti < MAX_TEAMS as usize && self.strokes[ti] > 0 {
                    self.strokes[ti] -= 1;
                }
                self.set_center_announcement("BALL LOST — REPLAY SHOT".into());
                log::line(&format!(
                    "[wkGolf] ball lost ({label}) — stroke {n} void, replay prior spot"
                ));
                if let (Some(x), Some(y)) = (self.ball_x[ti], self.ball_y[ti]) {
                    let _ = Self::teleport_worm(team, worm_idx, x, y, "prior ball");
                }
                self.after_shot_teleport(ti);
                return;
            }
        };

        let hole_idx = self.team_hole[ti];
        let h = &self.course.holes[hole_idx];
        let par = h.par;
        let in_cup = h.cup_contains(dx, dy);
        let (cup_x, cup_y) = h.cup_world();

        if !in_cup && !terrain::plausible_ball_pos(dx, dy) {
            if ti < MAX_TEAMS as usize && self.strokes[ti] > 0 {
                self.strokes[ti] -= 1;
            }
            self.set_center_announcement("BAD TRACK — REPLAY SHOT".into());
            log::line(&format!(
                "[wkGolf] bad det ({dx:.0},{dy:.0}) — stroke {n} void (off-map / stale track)"
            ));
            if let (Some(x), Some(y)) = (self.ball_x[ti], self.ball_y[ti]) {
                let _ = Self::teleport_worm(team, worm_idx, x, y, "prior ball");
            }
            self.after_shot_teleport(ti);
            return;
        }

        if in_cup {
            self.totals[ti] += n;
            let dist = hypot(dx - cup_x, dy - cup_y);
            let score = Self::hole_result(n, par);
            self.set_center_announcement(score.clone());
            log::line(&format!(
                "[wkGolf] HOLE IN! T{team} {n} strokes (par {par}) {score} dist={dist:.1}px"
            ));
            self.advance_team_hole(ti, team);
            if !self.course_done[ti] {
                self.pending_tee_snap[ti] = true;
                let prefer = wa_mem::current_turn()
                    .ok()
                    .filter(|(_, _, t, _)| *t == team)
                    .map(|(w, _, _, _)| w);
                if !self.try_tee_snap_for_team(team, prefer) {
                    log::line(&format!(
                        "[wkGolf] T{team} tee-snap queued — will retry on turn/skip"
                    ));
                }
            }
            return;
        }

        self.ball_x[ti] = Some(dx);
        self.ball_y[ti] = Some(dy);
        let _ = Self::teleport_worm(team, worm_idx, dx, dy, "det");
        self.after_shot_teleport(ti);
    }

    fn draw_hud(&mut self) {
        let Some((team, worm_idx)) = self.active_turn_worm() else {
            return;
        };
        if team >= MAX_TEAMS {
            return;
        }
        let ti = team as usize;
        let hole_n = self.team_hole[ti] + 1;
        let h = &self.course.holes[self.team_hole[ti]];
        let par = h.par;
        let strokes = self.strokes[ti];
        let player = Self::worm_name_for_turn(team, worm_idx);

        let mid_shot = self.pending_dets[ti].is_some();
        let shot_n = if mid_shot { strokes } else { strokes + 1 };

        let (player_line, shot_line, status_line) = if self.course_done[ti] {
            (
                player.to_uppercase(),
                String::new(),
                format!("FINISHED — TOTAL {}", self.totals[ti]),
            )
        } else if self.pending_tee_snap[ti] {
            (
                player.to_uppercase(),
                "TEEING UP".into(),
                String::new(),
            )
        } else {
            (
                player.to_uppercase(),
                Self::ordinal_shot(shot_n),
                Self::play_status(strokes, par),
            )
        };

        let layout = draw::GolfHudLayout {
            hole_title: format!("HOLE {hole_n}"),
            par_line: format!("PAR {par}"),
            spin_line: if self.course_done[ti] {
                String::new()
            } else {
                spin::get(team).hud_label().into()
            },
            player_line,
            shot_line,
            status_line,
            center_line: self.active_center_msg(),
        };
        draw::set_hud_layout(layout);
    }
}

fn hypot(x: f64, y: f64) -> f64 {
    (x * x + y * y).sqrt()
}
