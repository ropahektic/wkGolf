//! Read WA land/aim cursor via SharedData only ? never call into WA.
//!
//! Team land cursor (Teleport / girder / etc.) lives on the World object at
//! +0x7380 / +0x7384 (Fixed 16.16). Confirmed via TeamEntity__process_cursor /
//! TeamEntity__render_cursor (Ghidra). Worm +0x2E0/+0x2E4 are weapon params and
//! stay stale ? do NOT use them as the aim point.

use std::sync::atomic::{AtomicU32, Ordering};

use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{
    VirtualProtect, VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_NOACCESS,
    PAGE_EXECUTE, PAGE_READONLY, PAGE_READWRITE, PAGE_WRITECOPY, PAGE_EXECUTE_READ,
    PAGE_EXECUTE_READWRITE, PAGE_EXECUTE_WRITECOPY,
};

const IMAGE_BASE: u32 = 0x00400000;
const G_GAME_SESSION: u32 = 0x007A0884;
/// OpenWA `g_InGameSessionFlag` — 1 while `Frontend__LaunchGameSession` is active.
const G_IN_GAME_SESSION_FLAG: u32 = 0x007A083E;
/// OpenWA `g_CurrentScreen` — frontend navigation id (see `ScreenId`).
const G_CURRENT_SCREEN: u32 = 0x006B3504;
/// `GameSession.exit_flag` — nonzero ends the in-game main loop.
const OFF_SESSION_EXIT_FLAG: usize = 0x3C;
/// `GameSession.flag_40` — game-end status/result code.
const OFF_SESSION_END_STATUS: usize = 0x40;
/// `GameSession.frame_state` — `< 0` while headless / no render path.
const OFF_SESSION_FRAME_STATE: usize = 0x64;

const OFF_SESSION_RUNTIME: usize = 0xA0;
const OFF_RUNTIME_WORLD_ROOT: usize = 0x8;
/// `GameRuntime` → live DDGame / world used for HUD textbox (+0x544).
pub const OFF_RUNTIME_WORLD: usize = 0x488;
/// Render queue for `RQ_DrawSpriteLocal` (Jelly `GameGlobal+0x524`).
pub const OFF_WORLD_RENDER_QUEUE: usize = 0x524;

const OFF_BASE_SHARED_DATA: usize = 0x24;
const OFF_BASE_WORLD: usize = 0x2C;

const OFF_ROOT_TEAM_IDX: usize = 0x12C;
const OFF_WORLD_TEAM_STRIDE: usize = 0x51C;
const OFF_WORLD_ACTIVE_WORM: usize = 0x4620;

/// Team land cursor (Fixed) ? same coords RQ_DrawSpriteLocal uses for Teleport.
const OFF_WORLD_LAND_CURSOR_X: usize = 0x7380;
const OFF_WORLD_LAND_CURSOR_Y: usize = 0x7384;
/// Last committed land-cursor position (airstrike X after click).
const OFF_WORLD_LAND_CURSOR_X_LAST: usize = 0x7388;
const OFF_WORLD_LAND_CURSOR_Y_LAST: usize = 0x738c;
/// `TeamEntity__render_cursor` / `TeamEntity__reset_cursor`.
const OFF_TEAM_CURSOR_MOVE: usize = 0x3d0;
const OFF_TEAM_CURSOR_SPRITE: usize = 0x3d4;
const OFF_TEAM_CURSOR_MARK: usize = 0x3e0;
/// Stock markerr red X (`TeamEntity__render_cursor` / airstrike drop marker).
pub const SPRITE_MARKERR: u32 = 38;
/// Foreground scroll origin — `RQ_TranslateCoordinates` / `TeamEntity__render_cursor`.
const OFF_WORLD_RENDER_ORIGIN_X: usize = 0x8cec;
const OFF_WORLD_RENDER_ORIGIN_Y: usize = 0x8cf4;

const OFF_WORM_POS_X: usize = 0x84;
const OFF_WORM_POS_Y: usize = 0x88;
pub const OFF_WORM_TEAM: usize = 0xFC;
pub const OFF_WORM_IDX: usize = 0x100;
const OFF_WORM_TURN_ACTIVE: usize = 0x104;
const OFF_WORM_SHOW_CURSOR: usize = 0x268;
/// WormEntity aim angle (16.16) ? control_aim / release_focus read this (+0x24c).
const OFF_WORM_AIM_DISPLAY: usize = 0x24c;
/// Smoothed crosshair (+0x270). Visual only ? WeaponRelease uses +0x24c via release_focus.
const OFF_WORM_AIM_SMOOTH: usize = 0x270;
/// Charge power 16.16 ? WeaponRelease multiplies aim by this (+0x248).
const OFF_WORM_POWER: usize = 0x248;
/// Facing sign (+0x1a8). <0 ? face left (release_focus mirrors when <0).
const OFF_WORM_FACING_SIGN: usize = 0x1a8;
/// Bounce mode (+0x2c0): 0=min 30%, 1=max 60% (SelectBounce / WeaponRelease).
const OFF_WORM_BOUNCE_MODE: usize = 0x2c0;
/// Low Gravity spawn flag (`WeaponRelease` reads this at fire; cleared in `ClearWeaponState`).
const OFF_WORM_LOW_GRAVITY: usize = 0x2cc;
const OFF_WORM_WORLD: usize = 0x2c;
/// Team utility bitmask — `WormEntity__fire_utility` ORs a bit here when LG utility fires.
const OFF_WORLD_TEAM_UTIL: usize = 0x4628;
const OFF_WORLD_WEAPON_TABLE: usize = 0x510;
const WEAPON_REC_STRIDE: usize = 0x1d0;
/// Utility bit index for this weapon row (`fire_utility`: `1 << (*+0x3c & 0x1f)`).
const OFF_WEAPON_UTIL_BIT_IDX: usize = 0x3c;
/// Stock scheme weapon id (`Weapon_LowGravity` / Jelly enum).
const WEAPON_ID_LOW_GRAVITY: u32 = 63;
/// Stock LG: scheme projectile gravity × 1/2 (wkAimbot `WA_LOW_GRAVITY_SCALE`).
pub const WA_LOW_GRAVITY_SCALE: f64 = 0.5;
/// TeamArena per-worm HP (`WormEntity__ApplyDamage` / `CommitPendingHealth`).
const OFF_ARENA_WORM_HP: usize = 0x40EC;
const MAX_TEAMS: u32 = 6;
const MAX_WORMS_PER_TEAM: u32 = 8;

const KEY_WORM_ENTITY: u32 = 0x02;
/// SharedData key for `TeamEntity` (`TeamEntity__Constructor` inserts EDI=0x15, ESI=team).
const KEY_TEAM_ENTITY: u32 = 0x15;
/// Worm display name (`WormEntity__BroadcastWeaponName` / ctor @ +0x2F0).
const OFF_WORM_NAME: usize = 0x2F0;
const WORM_NAME_LEN: usize = 0x11;

#[derive(Clone, Copy, Debug)]
pub struct WaAimSample {
    pub worm_x: f64,
    pub worm_y: f64,
    pub cursor_x: f64,
    pub cursor_y: f64,
    pub rel_x: f64,
    pub rel_y: f64,
    pub show_cursor: u32,
    pub turn_active: u32,
    pub team_idx: u32,
    pub worm_idx: u32,
}

#[derive(Clone, Debug)]
pub struct WormInfo {
    pub ptr: usize,
    pub team: u32,
    pub worm_idx: u32,
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub hp: i32,
    pub turn_active: bool,
}

fn module_base() -> u32 {
    unsafe { GetModuleHandleW(std::ptr::null()) as u32 }
}

fn rb(ghidra_va: u32) -> usize {
    let base = module_base();
    ghidra_va.wrapping_add(base.wrapping_sub(IMAGE_BASE)) as usize
}

/// Map a Ghidra VA (image base `0x400000`) to the live module address.
pub fn ghidra_va(ghidra_va: u32) -> usize {
    rb(ghidra_va)
}

/// Scan committed readable/writable regions for a pointer-sized value (e.g. vtable).
/// Returns the address *of the object* (where the vtable pointer is stored).
pub fn scan_for_ptr(needle: usize) -> Option<usize> {
    if needle == 0 {
        return None;
    }
    unsafe {
        let mut addr = 0x10000usize;
        let mut mbi = std::mem::zeroed::<MEMORY_BASIC_INFORMATION>();
        while addr < 0x7FFF_0000 {
            let n = VirtualQuery(
                addr as *const _,
                &mut mbi,
                std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
            );
            if n == 0 {
                break;
            }
            let base = mbi.BaseAddress as usize;
            let size = mbi.RegionSize;
            let prot = mbi.Protect & 0xFF;
            let rw = matches!(
                prot,
                PAGE_READWRITE | PAGE_WRITECOPY | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY
            );
            if (mbi.State & MEM_COMMIT) != 0 && rw && size >= 4 && size < 64 * 1024 * 1024 {
                let mut off = 0usize;
                while off + 4 <= size {
                    let p = base + off;
                    if let Some(v) = read_usize(p) {
                        if v == needle {
                            return Some(p);
                        }
                    }
                    off += 4;
                }
            }
            let next = base.saturating_add(size);
            if next <= addr {
                break;
            }
            addr = next;
        }
    }
    None
}

fn readable(addr: usize, size: usize) -> bool {
    if addr == 0 || addr > 0xFFFF_FFF0 {
        return false;
    }
    unsafe {
        let mut mbi = std::mem::zeroed::<MEMORY_BASIC_INFORMATION>();
        let n = VirtualQuery(
            addr as *const _,
            &mut mbi,
            std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
        );
        if n == 0 {
            return false;
        }
        if (mbi.State & MEM_COMMIT) == 0 {
            return false;
        }
        let prot = mbi.Protect & 0xFF;
        let ok = matches!(
            prot,
            PAGE_READONLY
                | PAGE_READWRITE
                | PAGE_WRITECOPY
                | PAGE_EXECUTE_READ
                | PAGE_EXECUTE_READWRITE
                | PAGE_EXECUTE_WRITECOPY
        );
        if !ok || prot == PAGE_NOACCESS || prot == PAGE_EXECUTE {
            return false;
        }
        let end = addr.saturating_add(size);
        let region_end = (mbi.BaseAddress as usize).saturating_add(mbi.RegionSize);
        end <= region_end
    }
}

unsafe fn read_usize(addr: usize) -> Option<usize> {
    if !readable(addr, 4) {
        return None;
    }
    Some(*(addr as *const usize))
}

unsafe fn read_u32(addr: usize) -> Option<u32> {
    if !readable(addr, 4) {
        return None;
    }
    Some(*(addr as *const u32))
}

unsafe fn read_i32(addr: usize) -> Option<i32> {
    if !readable(addr, 4) {
        return None;
    }
    Some(*(addr as *const i32))
}

unsafe fn read_u8(addr: usize) -> Option<u8> {
    if !readable(addr, 1) {
        return None;
    }
    Some(*(addr as *const u8))
}

// --- pub helpers for terrain snapshot (read-only) ---

pub fn active_world() -> Result<usize, &'static str> {
    unsafe { Ok(resolve_active_worm()?.1) }
}

pub fn ptr_readable(addr: usize, size: usize) -> bool {
    readable(addr, size)
}

pub fn read_ptr(addr: usize) -> Option<usize> {
    unsafe { read_usize(addr) }
}

pub fn read_u32_at(addr: usize) -> Option<u32> {
    unsafe { read_u32(addr) }
}

pub fn read_i32_at(addr: usize) -> Option<i32> {
    unsafe { read_i32(addr) }
}

pub fn read_i16_at(addr: usize) -> Option<i16> {
    unsafe {
        if !readable(addr, 2) {
            return None;
        }
        Some(std::ptr::read_unaligned(addr as *const i16))
    }
}

pub fn read_u8_at(addr: usize) -> Option<u8> {
    unsafe { read_u8(addr) }
}

/// Stock worm name string (e.g. "Rusty", "Cpt. Molo").
pub fn read_worm_name(ptr: usize) -> String {
    if ptr == 0 {
        return String::new();
    }
    let mut out = Vec::new();
    for i in 0..WORM_NAME_LEN {
        let Some(ch) = read_u8_at(ptr + OFF_WORM_NAME + i) else {
            break;
        };
        if ch == 0 {
            break;
        }
        out.push(ch);
    }
    String::from_utf8_lossy(&out).trim().to_string()
}

fn writable(addr: usize, size: usize) -> bool {
    if addr == 0 || addr > 0xFFFF_FFF0 {
        return false;
    }
    unsafe {
        let mut mbi = std::mem::zeroed::<MEMORY_BASIC_INFORMATION>();
        let n = VirtualQuery(
            addr as *const _,
            &mut mbi,
            std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
        );
        if n == 0 || (mbi.State & MEM_COMMIT) == 0 {
            return false;
        }
        let prot = mbi.Protect & 0xFF;
        matches!(
            prot,
            PAGE_READWRITE | PAGE_WRITECOPY | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY
        ) && (addr + size) <= (mbi.BaseAddress as usize).saturating_add(mbi.RegionSize)
    }
}

pub fn write_u8_at(addr: usize, val: u8) -> bool {
    if !writable(addr, 1) {
        return false;
    }
    unsafe {
        *(addr as *mut u8) = val;
    }
    true
}

/// Write bytes into a writable (or VirtualProtect-lifted) region.
pub fn write_bytes_at(addr: usize, bytes: &[u8]) -> bool {
    if addr == 0 || bytes.is_empty() {
        return false;
    }
    unsafe {
        let mut old = 0u32;
        let ok = VirtualProtect(
            addr as *const _,
            bytes.len(),
            PAGE_EXECUTE_READWRITE,
            &mut old,
        );
        if ok == 0 && !writable(addr, bytes.len()) {
            return false;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), addr as *mut u8, bytes.len());
        if ok != 0 {
            let mut ignore = 0u32;
            let _ = VirtualProtect(addr as *const _, bytes.len(), old, &mut ignore);
        }
        true
    }
}

pub fn write_cstr_at(addr: usize, s: &str, max_incl_nul: usize) -> bool {
    if max_incl_nul == 0 {
        return false;
    }
    let mut buf: Vec<u8> = s.bytes().take(max_incl_nul - 1).collect();
    buf.push(0);
    write_bytes_at(addr, &buf)
}

fn fixed_to_f64(raw: i32) -> f64 {
    raw as f64 / 65536.0
}

fn shared_bucket(key_esi: u32, key_edi: u32) -> usize {
    ((key_esi.wrapping_mul(0x11).wrapping_add(key_edi)) & 0xFF) as usize
}

unsafe fn shared_lookup(shared_data: usize, key_esi: u32, key_edi: u32) -> Option<usize> {
    if shared_data == 0 || !readable(shared_data, 0x400) {
        return None;
    }
    let bucket = shared_bucket(key_esi, key_edi);
    let mut node = read_usize(shared_data + bucket * 4)?;
    let mut hops = 0u32;
    while node != 0 && hops < 64 {
        hops += 1;
        if !readable(node, 0x10) {
            return None;
        }
        let n_edi = read_u32(node + 4)?;
        let n_esi = read_u32(node + 8)?;
        if n_edi == key_edi && n_esi == key_esi {
            let ent = read_usize(node + 0xC)?;
            if ent != 0 && readable(ent, 0x100) {
                return Some(ent);
            }
            return None;
        }
        node = read_usize(node)?;
    }
    None
}

unsafe fn resolve_active_worm() -> Result<(usize, usize, u32, u32), &'static str> {
    let slot = rb(G_GAME_SESSION);
    let session = read_usize(slot).ok_or("bad session slot")?;
    if session == 0 {
        return Err("null GameSession");
    }
    let runtime = read_usize(session + OFF_SESSION_RUNTIME).ok_or("bad runtime ptr")?;
    if runtime == 0 {
        return Err("null GameRuntime");
    }
    let world_root = read_usize(runtime + OFF_RUNTIME_WORLD_ROOT).ok_or("bad world_root ptr")?;
    if world_root == 0 {
        return Err("null WorldRoot");
    }
    if !readable(world_root, 0x140) {
        return Err("WorldRoot not readable");
    }

    let shared = read_usize(world_root + OFF_BASE_SHARED_DATA).ok_or("bad shared_data")?;
    let world = read_usize(world_root + OFF_BASE_WORLD).ok_or("bad world ptr")?;
    if world == 0 {
        return Err("null world");
    }
    if !readable(world, 0x7390) {
        return Err("world cursor region not readable");
    }

    let team_idx = read_u32(world_root + OFF_ROOT_TEAM_IDX).ok_or("bad team idx")?;
    let team_off = (team_idx as usize).wrapping_mul(OFF_WORLD_TEAM_STRIDE);
    let worm_idx =
        read_u32(world + OFF_WORLD_ACTIVE_WORM + team_off).ok_or("bad active worm idx")?;

    let key_esi = team_idx.wrapping_mul(16).wrapping_add(worm_idx);
    let worm = shared_lookup(shared, key_esi, KEY_WORM_ENTITY)
        .ok_or("SharedData worm lookup miss")?;
    Ok((worm, world, team_idx, worm_idx))
}

/// `(worm_ptr, world, team_idx, worm_idx)` for the WorldRoot current turn.
pub fn current_turn() -> Result<(usize, usize, u32, u32), &'static str> {
    unsafe { resolve_active_worm() }
}

/// Live world / GameGlobal pointer during a match.
pub fn resolve_world() -> Result<usize, &'static str> {
    unsafe {
        let slot = rb(G_GAME_SESSION);
        let session = read_usize(slot).ok_or("bad session slot")?;
        if session == 0 {
            return Err("null GameSession");
        }
        let runtime = read_usize(session + OFF_SESSION_RUNTIME).ok_or("bad runtime ptr")?;
        if runtime == 0 {
            return Err("null GameRuntime");
        }
        let world_root = read_usize(runtime + OFF_RUNTIME_WORLD_ROOT).ok_or("bad world_root")?;
        if world_root == 0 {
            return Err("null WorldRoot");
        }
        let world = read_usize(world_root + OFF_BASE_WORLD).ok_or("bad world ptr")?;
        if world == 0 {
            return Err("null world");
        }
        Ok(world)
    }
}

/// DDGame world on `GameRuntime` (+0x488) — textbox/HUD path (not WorldRoot world).
pub fn resolve_runtime_world() -> Result<usize, &'static str> {
    unsafe {
        let slot = rb(G_GAME_SESSION);
        let session = read_usize(slot).ok_or("bad session slot")?;
        if session == 0 {
            return Err("null GameSession");
        }
        let runtime = read_usize(session + OFF_SESSION_RUNTIME).ok_or("bad runtime ptr")?;
        if runtime == 0 {
            return Err("null GameRuntime");
        }
        let world = read_usize(runtime + OFF_RUNTIME_WORLD).ok_or("bad runtime world")?;
        if world == 0 {
            return Err("null runtime world");
        }
        Ok(world)
    }
}

/// `RQ_DrawSpriteLocal` queue — `*(GameGlobal + 0x524)`, not DisplayGfx (+0x4).
pub fn resolve_render_queue() -> Result<usize, &'static str> {
    let world = resolve_runtime_world()?;
    read_ptr(world + OFF_WORLD_RENDER_QUEUE).ok_or("bad render queue ptr")
}

/// Cheap guard before touching queue slots from a render hook.
pub fn looks_like_render_queue(queue: usize) -> bool {
    if queue == 0 {
        return false;
    }
    if !readable(queue, 0x1000c) {
        return false;
    }
    let count = unsafe { read_u32(queue + 0x10004).unwrap_or(0xFFFF_FFFF) };
    count <= 0x800
}

/// Team/worm indices for the current turn even if SharedData worm ptr is missing.
pub fn current_turn_ids() -> Result<(u32, u32), &'static str> {
    unsafe {
        let slot = rb(G_GAME_SESSION);
        let session = read_usize(slot).ok_or("bad session slot")?;
        if session == 0 {
            return Err("null GameSession");
        }
        let runtime = read_usize(session + OFF_SESSION_RUNTIME).ok_or("bad runtime ptr")?;
        if runtime == 0 {
            return Err("null GameRuntime");
        }
        let world_root = read_usize(runtime + OFF_RUNTIME_WORLD_ROOT).ok_or("bad world_root")?;
        if world_root == 0 {
            return Err("null WorldRoot");
        }
        let world = read_usize(world_root + OFF_BASE_WORLD).ok_or("bad world")?;
        if world == 0 {
            return Err("null world");
        }
        let team_idx = read_u32(world_root + OFF_ROOT_TEAM_IDX).ok_or("bad team idx")?;
        let team_off = (team_idx as usize).wrapping_mul(OFF_WORLD_TEAM_STRIDE);
        let worm_idx =
            read_u32(world + OFF_WORLD_ACTIVE_WORM + team_off).ok_or("bad active worm idx")?;
        Ok((team_idx, worm_idx))
    }
}

/// Raw aim (+0x24c) and smoothed crosshair (+0x270). Fire uses raw.
pub fn read_aim_pair() -> Result<(u32, u32), &'static str> {
    unsafe {
        let (worm, _, _, _) = resolve_active_worm()?;
        let raw = read_i32(worm + OFF_WORM_AIM_DISPLAY).ok_or("bad aim +0x24c")? as u32;
        let smooth = read_i32(worm + OFF_WORM_AIM_SMOOTH).ok_or("bad aim +0x270")? as u32;
        Ok((raw, smooth))
    }
}

/// Live display aim from worm +0x24c.
pub fn read_display_aim() -> Result<u32, &'static str> {
    Ok(read_aim_pair()?.0)
}

pub fn read_worm_aim(worm: usize) -> Option<u32> {
    read_i32_at(worm + OFF_WORM_AIM_DISPLAY).map(|v| v as u32)
}

/// Smoothed crosshair (+0x270) — what the player sees while aiming.
pub fn read_worm_aim_smooth(worm: usize) -> Option<u32> {
    read_i32_at(worm + OFF_WORM_AIM_SMOOTH).map(|v| v as u32)
}

pub fn read_worm_facing(worm: usize) -> Option<i32> {
    read_i32_at(worm + OFF_WORM_FACING_SIGN)
}

/// Low Gravity active this turn — spawn flag (+0x2cc) or team utility bit (LG utility path).
pub fn read_worm_low_gravity(worm: usize) -> bool {
    if read_u32_at(worm + OFF_WORM_LOW_GRAVITY).unwrap_or(0) != 0 {
        return true;
    }
    read_team_low_gravity_util(worm)
}

fn read_team_low_gravity_util(worm: usize) -> bool {
    let Some(world) = read_ptr(worm + OFF_WORM_WORLD) else {
        return false;
    };
    let team = read_u32_at(worm + OFF_WORM_TEAM).unwrap_or(0);
    if team >= MAX_TEAMS {
        return false;
    }
    let flags_off = world + OFF_WORLD_TEAM_UTIL + team as usize * OFF_WORLD_TEAM_STRIDE;
    let Some(flags) = read_u32_at(flags_off) else {
        return false;
    };
    let lg_row = world + OFF_WORLD_WEAPON_TABLE + WEAPON_ID_LOW_GRAVITY as usize * WEAPON_REC_STRIDE;
    let Some(bit_idx) = read_u32_at(lg_row + OFF_WEAPON_UTIL_BIT_IDX) else {
        return false;
    };
    let mask = 1u32 << (bit_idx & 0x1f);
    (flags & mask) != 0
}

/// Live foreground scroll origin (Fixed @ +0x8cec / +0x8cf4).
pub fn read_render_origin(world: usize) -> Option<(i32, i32)> {
    let ox = read_i32_at(world + OFF_WORLD_RENDER_ORIGIN_X)?;
    let oy = read_i32_at(world + OFF_WORLD_RENDER_ORIGIN_Y)?;
    Some((ox, oy))
}

/// Stock `RQ_TranslateCoordinates` @ 0x00542B10 — subtract origin high words, saturate.
pub fn rq_translate_fixed(value: i32, origin: i32) -> i32 {
    let v = value & 0xffff0000u32 as i32;
    let o = origin & 0xffff0000u32 as i32;
    let diff = v.wrapping_sub(o);
    if v >= 0 {
        if o < 0 && diff <= 0 {
            return 0x7fff_ffff;
        }
    } else if o > 0 && diff >= 0 {
        return 0x8000_0000u32 as i32;
    }
    diff
}

/// Live charge bar (+0x248), normalized 0..1.
pub fn read_power() -> Result<f64, &'static str> {
    unsafe {
        let (worm, _, _, _) = resolve_active_worm()?;
        let raw = read_i32(worm + OFF_WORM_POWER).ok_or("bad power +0x248")?;
        Ok((raw as f64) / 65536.0)
    }
}

/// Stock facing at +0x1a8. Negative ? face left.
pub fn read_facing_sign() -> Result<i32, &'static str> {
    unsafe {
        let (worm, _, _, _) = resolve_active_worm()?;
        read_i32(worm + OFF_WORM_FACING_SIGN).ok_or("bad facing +0x1a8")
    }
}

/// Bounce toggle at +0x2c0: 0 = min, 1 = max (SelectBounce / WeaponRelease).
/// Stock WeaponRelease writes delay 0x1E (30) or 0x3C (60) from this flag —
/// our sim treats those as bounce_y 0.30 / 0.60 pending live missile verify.
pub fn read_bounce_max() -> Result<bool, &'static str> {
    unsafe {
        let (worm, _, _, _) = resolve_active_worm()?;
        let v = read_i32(worm + OFF_WORM_BOUNCE_MODE).ok_or("bad bounce +0x2c0")?;
        Ok(v != 0)
    }
}

/// Set worm +0x2c0 so WeaponRelease matches the planned min/max bounce.
pub fn write_bounce_max(want_max: bool) -> Result<(), &'static str> {
    unsafe {
        let (worm, _, _, _) = resolve_active_worm()?;
        let addr = worm + OFF_WORM_BOUNCE_MODE;
        let val: i32 = if want_max { 1 } else { 0 };
        let bytes = val.to_le_bytes();
        if !write_bytes_at(addr, &bytes) {
            return Err("write bounce +0x2c0 failed");
        }
        let got = read_i32(addr).ok_or("reread bounce +0x2c0")?;
        if (got != 0) != want_max {
            return Err("bounce +0x2c0 verify mismatch");
        }
        Ok(())
    }
}

/// Write team land cursor (Fixed 16.16) — Teleport / girder / airstrike weapons.
pub fn write_land_cursor(x: f64, y: f64) -> Result<(), &'static str> {
    let world = active_world()?;
    let xi = (x * 65536.0).round() as i32;
    let yi = (y * 65536.0).round() as i32;
    if !write_bytes_at(world + OFF_WORLD_LAND_CURSOR_X, &xi.to_le_bytes()) {
        return Err("write land cursor x failed");
    }
    if !write_bytes_at(world + OFF_WORLD_LAND_CURSOR_Y, &yi.to_le_bytes()) {
        return Err("write land cursor y failed");
    }
    Ok(())
}

/// Live `TeamEntity` for a team index (SharedData ESI=team, EDI=0x15).
pub fn resolve_team_entity(team: u32) -> Result<usize, &'static str> {
    unsafe {
        let slot = rb(G_GAME_SESSION);
        let session = read_usize(slot).ok_or("bad session slot")?;
        if session == 0 {
            return Err("null GameSession");
        }
        let runtime = read_usize(session + OFF_SESSION_RUNTIME).ok_or("bad runtime ptr")?;
        if runtime == 0 {
            return Err("null GameRuntime");
        }
        let world_root = read_usize(runtime + OFF_RUNTIME_WORLD_ROOT).ok_or("bad world_root")?;
        if world_root == 0 {
            return Err("null WorldRoot");
        }
        let shared = read_usize(world_root + OFF_BASE_SHARED_DATA).ok_or("bad shared_data")?;
        shared_lookup(shared, team, KEY_TEAM_ENTITY).ok_or("TeamEntity lookup miss")
    }
}

/// Show world-locked markerr via stock land cursor (`TeamEntity__render_cursor`).
pub fn show_land_mark(x: f64, y: f64, team: u32) -> Result<(), &'static str> {
    write_land_cursor(x, y)?;
    let world = active_world()?;
    let xi = (x * 65536.0).round() as i32;
    let yi = (y * 65536.0).round() as i32;
    if !write_bytes_at(world + OFF_WORLD_LAND_CURSOR_X_LAST, &xi.to_le_bytes()) {
        return Err("write land cursor x last failed");
    }
    if !write_bytes_at(world + OFF_WORLD_LAND_CURSOR_Y_LAST, &yi.to_le_bytes()) {
        return Err("write land cursor y last failed");
    }
    let team_ent = resolve_team_entity(team)?;
    if !write_u32_at(team_ent + OFF_TEAM_CURSOR_MOVE, 0) {
        return Err("clear team cursor move failed");
    }
    if !write_u32_at(team_ent + OFF_TEAM_CURSOR_MARK, 1) {
        return Err("set team cursor mark failed");
    }
    if !write_u32_at(team_ent + OFF_TEAM_CURSOR_SPRITE, SPRITE_MARKERR) {
        return Err("set team cursor sprite failed");
    }
    Ok(())
}

/// Hide stock land-cursor mark (`TeamEntity__reset_cursor` without handle teardown).
pub fn hide_land_mark(team: u32) -> Result<(), &'static str> {
    let team_ent = resolve_team_entity(team)?;
    if !write_u32_at(team_ent + OFF_TEAM_CURSOR_MOVE, 0) {
        return Err("clear team cursor move failed");
    }
    if !write_u32_at(team_ent + OFF_TEAM_CURSOR_MARK, 0) {
        return Err("clear team cursor mark failed");
    }
    Ok(())
}

fn write_u32_at(addr: usize, val: u32) -> bool {
    write_bytes_at(addr, &val.to_le_bytes())
}

pub fn sample_aim_cursor() -> Result<WaAimSample, &'static str> {
    unsafe {
        let (worm, world, team_idx, worm_idx) = resolve_active_worm()?;

        let turn_active = read_u32(worm + OFF_WORM_TURN_ACTIVE).unwrap_or(0);
        let show_cursor = read_u32(worm + OFF_WORM_SHOW_CURSOR).unwrap_or(0);
        let worm_x = fixed_to_f64(read_i32(worm + OFF_WORM_POS_X).ok_or("bad pos x")?);
        let worm_y = fixed_to_f64(read_i32(worm + OFF_WORM_POS_Y).ok_or("bad pos y")?);

        // Team land cursor ? updates while Teleport (etc.) marker moves.
        let cursor_x =
            fixed_to_f64(read_i32(world + OFF_WORLD_LAND_CURSOR_X).ok_or("bad land cursor x")?);
        let cursor_y =
            fixed_to_f64(read_i32(world + OFF_WORLD_LAND_CURSOR_Y).ok_or("bad land cursor y")?);

        Ok(WaAimSample {
            worm_x,
            worm_y,
            cursor_x,
            cursor_y,
            rel_x: cursor_x - worm_x,
            rel_y: cursor_y - worm_y,
            show_cursor,
            turn_active,
            team_idx,
            worm_idx,
        })
    }
}

fn arena_worm_hp(world: usize, team: u32, worm_idx: u32) -> Option<i32> {
    let addr = world
        + (team as usize) * OFF_WORLD_TEAM_STRIDE
        + (worm_idx as usize) * 0x9C
        + OFF_ARENA_WORM_HP;
    unsafe { read_i32(addr) }
}

/// Every WormEntity in SharedData (ignores arena HP — entity may still exist).
pub fn list_worm_entities() -> Result<Vec<WormInfo>, &'static str> {
    unsafe {
        let slot = rb(G_GAME_SESSION);
        let session = read_usize(slot).ok_or("bad session slot")?;
        if session == 0 {
            return Err("null GameSession");
        }
        let runtime = read_usize(session + OFF_SESSION_RUNTIME).ok_or("bad runtime ptr")?;
        if runtime == 0 {
            return Err("null GameRuntime");
        }
        let world_root = read_usize(runtime + OFF_RUNTIME_WORLD_ROOT).ok_or("bad world_root")?;
        if world_root == 0 {
            return Err("null WorldRoot");
        }
        let shared = read_usize(world_root + OFF_BASE_SHARED_DATA).ok_or("bad shared_data")?;
        let world = read_usize(world_root + OFF_BASE_WORLD).ok_or("bad world ptr")?;
        if world == 0 {
            return Err("null world");
        }

        let mut out = Vec::new();
        for team in 0..MAX_TEAMS {
            for worm_idx in 0..MAX_WORMS_PER_TEAM {
                let key_esi = team.wrapping_mul(16).wrapping_add(worm_idx);
                let Some(ptr) = shared_lookup(shared, key_esi, KEY_WORM_ENTITY) else {
                    continue;
                };
                let t = read_u32(ptr + OFF_WORM_TEAM).unwrap_or(team);
                let w = read_u32(ptr + OFF_WORM_IDX).unwrap_or(worm_idx);
                let hp = arena_worm_hp(world, t, w).unwrap_or(0);
                let x = fixed_to_f64(read_i32(ptr + OFF_WORM_POS_X).ok_or("worm x")?);
                let y = fixed_to_f64(read_i32(ptr + OFF_WORM_POS_Y).ok_or("worm y")?);
                let turn = read_u32(ptr + OFF_WORM_TURN_ACTIVE).unwrap_or(0) != 0;
                let name = read_worm_name(ptr);
                out.push(WormInfo {
                    ptr,
                    team: t,
                    worm_idx: w,
                    name,
                    x,
                    y,
                    hp,
                    turn_active: turn,
                });
            }
        }
        Ok(out)
    }
}

pub fn format_worm_entity_census() -> String {
    let Ok(worms) = list_worm_entities() else {
        return "entities=?".into();
    };
    if worms.is_empty() {
        return "entities=none".into();
    }
    worms
        .iter()
        .map(|w| format!("T{}w{}", w.team, w.worm_idx))
        .collect::<Vec<_>>()
        .join(",")
}

const G_GAME_INFO: u32 = 0x0077_49A0;
const GI_TEAM_REC_COUNT: usize = 0x44C;
const GI_TEAM_RECORDS: usize = 0x450;
const GI_TEAM_REC_STRIDE: usize = 0xBB8;
const GI_REC_ELIMINATED: usize = 0x2;

/// Read-only GameInfo team-record eliminated flags (not HP — roster state).
pub fn format_gameinfo_roster() -> String {
    let Some(gi) = read_ptr(ghidra_va(G_GAME_INFO)) else {
        return "gi=?".into();
    };
    let count = read_u8_at(gi + GI_TEAM_REC_COUNT).unwrap_or(0).min(6);
    if count == 0 {
        return "gi=none".into();
    }
    let mut parts = Vec::new();
    for i in 0..count as usize {
        let rec = gi + GI_TEAM_RECORDS + i * GI_TEAM_REC_STRIDE;
        let elim = read_u8_at(rec + GI_REC_ELIMINATED).unwrap_or(0);
        parts.push(format!("r{i}:e{elim}"));
    }
    format!("gi=[{}]", parts.join(","))
}

/// All alive worms currently in SharedData (HP &gt; 0).
/// Fixed 16.16 worm position (+0x84/+0x88).
pub fn read_worm_pos(worm: usize) -> Option<(f64, f64)> {
    let x = fixed_to_f64(read_i32_at(worm + OFF_WORM_POS_X)?);
    let y = fixed_to_f64(read_i32_at(worm + OFF_WORM_POS_Y)?);
    Some((x, y))
}

pub fn read_worm_turn_active(worm: usize) -> bool {
    unsafe { read_u32(worm + OFF_WORM_TURN_ACTIVE).unwrap_or(0) != 0 }
}

pub fn list_alive_worms() -> Result<Vec<WormInfo>, &'static str> {
    unsafe {
        let slot = rb(G_GAME_SESSION);
        let session = read_usize(slot).ok_or("bad session slot")?;
        if session == 0 {
            return Err("null GameSession");
        }
        let runtime = read_usize(session + OFF_SESSION_RUNTIME).ok_or("bad runtime ptr")?;
        if runtime == 0 {
            return Err("null GameRuntime");
        }
        let world_root = read_usize(runtime + OFF_RUNTIME_WORLD_ROOT).ok_or("bad world_root")?;
        if world_root == 0 {
            return Err("null WorldRoot");
        }
        let shared = read_usize(world_root + OFF_BASE_SHARED_DATA).ok_or("bad shared_data")?;
        let world = read_usize(world_root + OFF_BASE_WORLD).ok_or("bad world ptr")?;
        if world == 0 {
            return Err("null world");
        }

        let mut out = Vec::new();
        for team in 0..MAX_TEAMS {
            for worm_idx in 0..MAX_WORMS_PER_TEAM {
                let hp = arena_worm_hp(world, team, worm_idx).unwrap_or(0);
                if hp <= 0 {
                    continue;
                }
                let key_esi = team.wrapping_mul(16).wrapping_add(worm_idx);
                let Some(ptr) = shared_lookup(shared, key_esi, KEY_WORM_ENTITY) else {
                    continue;
                };
                let x = fixed_to_f64(read_i32(ptr + OFF_WORM_POS_X).ok_or("worm x")?);
                let y = fixed_to_f64(read_i32(ptr + OFF_WORM_POS_Y).ok_or("worm y")?);
                let turn = read_u32(ptr + OFF_WORM_TURN_ACTIVE).unwrap_or(0) != 0;
                // Prefer entity's own team/idx fields when present.
                let t = read_u32(ptr + OFF_WORM_TEAM).unwrap_or(team);
                let w = read_u32(ptr + OFF_WORM_IDX).unwrap_or(worm_idx);
                let name = read_worm_name(ptr);
                out.push(WormInfo {
                    ptr,
                    team: t,
                    worm_idx: w,
                    name,
                    x,
                    y,
                    hp,
                    turn_active: turn,
                });
            }
        }
        Ok(out)
    }
}

/// `CPlayers` global — `GetTotalWorms` / `GetFirstTeamByMachine` use this.
const GHIDRA_CPLAYERS: u32 = 0x0087_79D8;
const OFF_CPLAYERS_TEAM0: usize = 0x624;
const OFF_TEAM_MACHINE: usize = 0x0;
/// Occupied / present — same as `GetFirstTeamByMachine` `[ECX+0x124]`.
const OFF_TEAM_ROW_PRESENT: usize = 0x124;
const TEAM_STRIDE_PLAYERS: usize = 0xD7B;

fn team_row(team: u32) -> Option<usize> {
    if team >= MAX_TEAMS {
        return None;
    }
    Some(rb(GHIDRA_CPLAYERS) + OFF_CPLAYERS_TEAM0 + team as usize * TEAM_STRIDE_PLAYERS)
}

fn team_machine_id(team: u32) -> Option<i32> {
    let row = team_row(team)?;
    unsafe { read_u8(row + OFF_TEAM_MACHINE).map(|b| b as i32) }
}

fn team_row_present(team: u32) -> bool {
    let Some(row) = team_row(team) else {
        return false;
    };
    unsafe { read_u8(row + OFF_TEAM_ROW_PRESENT).unwrap_or(0) != 0 }
}

/// CPlayers lobby slots whose machine id is 0 (this client) — slot ids, not in-game.
pub fn local_team_indices() -> Vec<u32> {
    let mut out = Vec::new();
    for t in 0..MAX_TEAMS {
        if team_row_present(t) && matches!(team_machine_id(t), Some(0)) {
            out.push(t);
        }
    }
    out
}

pub fn format_team_machines() -> String {
    let mut parts = Vec::new();
    for t in 0..MAX_TEAMS {
        if team_row_present(t) {
            let mid = team_machine_id(t).unwrap_or(-1);
            parts.push(format!("T{t}=m{mid}"));
        }
    }
    if parts.is_empty() {
        "teams=none".into()
    } else {
        parts.join(" ")
    }
}

/// Map CPlayers machine-0 slots → in-game team ids (WorldRoot / WormEntity).
/// Lobby slots are often 0,1 while live teams are 1,2 — zip sorted lists.
pub fn local_ingame_teams(worms: &[WormInfo]) -> Result<Vec<u32>, &'static str> {
    let mut cps: Vec<(u32, i32)> = Vec::new();
    for t in 0..MAX_TEAMS {
        if team_row_present(t) {
            cps.push((t, team_machine_id(t).unwrap_or(-1)));
        }
    }
    cps.sort_by_key(|(t, _)| *t);

    let mut ingame: Vec<u32> = Vec::new();
    for w in worms {
        if !ingame.contains(&w.team) {
            ingame.push(w.team);
        }
    }
    ingame.sort_unstable();

    if cps.is_empty() || ingame.is_empty() {
        return Err("no teams to map");
    }

    let mut locals = Vec::new();
    if cps.len() == ingame.len() {
        for (i, (_slot, mid)) in cps.iter().enumerate() {
            if *mid == 0 {
                locals.push(ingame[i]);
            }
        }
    } else {
        for (slot, mid) in &cps {
            if *mid != 0 {
                continue;
            }
            if ingame.contains(slot) {
                locals.push(*slot);
            } else if ingame.contains(&(slot + 1)) {
                locals.push(slot + 1);
            }
        }
    }
    Ok(locals)
}

pub fn format_turn_map() -> String {
    let Ok(worms) = list_worm_entities() else {
        return "map=?".into();
    };
    let mut wt: Vec<u32> = worms.iter().map(|w| w.team).collect();
    wt.sort_unstable();
    wt.dedup();
    let locals = local_ingame_teams(&worms).unwrap_or_default();
    let turn = current_turn_ids()
        .map(|(t, w)| format!("turn=T{t}w{w}"))
        .unwrap_or_else(|_| "turn=?".into());
    format!(
        "{turn} locals_ingame=[{}] alive_teams=[{}] cps=({})",
        locals
            .iter()
            .map(|t| t.to_string())
            .collect::<Vec<_>>()
            .join(","),
        wt.iter()
            .map(|t| t.to_string())
            .collect::<Vec<_>>()
            .join(","),
        format_team_machines()
    )
}

pub fn is_local_team(team: u32) -> bool {
    let Ok(worms) = list_alive_worms() else {
        return team_row_present(team) && matches!(team_machine_id(team), Some(0));
    };
    local_ingame_teams(&worms)
        .map(|v| v.contains(&team))
        .unwrap_or(false)
}

/// True when any occupied CPlayers row has machine id ≠ 0 (WormNET / remote peer).
pub fn is_network_session() -> bool {
    for t in 0..MAX_TEAMS {
        if team_row_present(t) {
            if let Some(mid) = team_machine_id(t) {
                if mid != 0 {
                    return true;
                }
            }
        }
    }
    false
}

/// One-line roster for logs: machines + which in-game teams this client owns.
pub fn format_network_roster() -> String {
    let net = if is_network_session() { "net" } else { "hotseat" };
    let worms = list_alive_worms().unwrap_or_default();
    let locals = local_ingame_teams(&worms).unwrap_or_default();
    format!(
        "mode={net} {} locals_ingame=[{}]",
        format_team_machines(),
        locals
            .iter()
            .map(|t| t.to_string())
            .collect::<Vec<_>>()
            .join(",")
    )
}

pub fn in_game_duel_ready() -> bool {
    let Ok(worms) = list_alive_worms() else {
        return false;
    };
    let mut teams = [false; MAX_TEAMS as usize];
    let mut n = 0u32;
    for w in &worms {
        if (w.team as usize) < teams.len() && !teams[w.team as usize] {
            teams[w.team as usize] = true;
            n += 1;
        }
    }
    n >= 2 && worms.len() >= 2
}

/// Live `GameSession*` from `G_GAME_SESSION` (null when not in a match run).
pub fn game_session_ptr() -> Option<usize> {
    let slot = ghidra_va(G_GAME_SESSION);
    let p = read_ptr(slot)?;
    if p == 0 {
        None
    } else {
        Some(p)
    }
}

/// True while the game session is running (death/results still count — not worm HP).
pub fn in_game_session() -> bool {
    if game_session_ptr().is_some() {
        return true;
    }
    // Flag can lag a frame either side of the pointer; treat either as in-session.
    read_u8_at(ghidra_va(G_IN_GAME_SESSION_FLAG)).unwrap_or(0) != 0
}

/// OpenWA headful gate: `GameSession.frame_state >= 0` before GameRender draws.
pub fn headful_frame_active() -> bool {
    let Some(s) = game_session_ptr() else {
        return false;
    };
    read_i32_at(s + OFF_SESSION_FRAME_STATE).unwrap_or(-1) >= 0
}

/// Frontend `g_CurrentScreen` (OpenWA `ScreenId` / navigation loop).
pub fn frontend_screen_id() -> Option<u32> {
    read_u32_at(ghidra_va(G_CURRENT_SCREEN))
}

/// `(exit_flag, end_status)` from the live GameSession, if any.
pub fn game_session_end_flags() -> Option<(u32, u32)> {
    let s = game_session_ptr()?;
    let exit = read_u32_at(s + OFF_SESSION_EXIT_FLAG)?;
    let status = read_u32_at(s + OFF_SESSION_END_STATUS)?;
    Some((exit, status))
}

/// Active local turn worm + all living enemy worms, nearest first.
pub fn duel_context() -> Result<(WormInfo, Vec<WormInfo>), &'static str> {
    let worms = list_alive_worms()?;
    if worms.len() < 2 {
        return Err("need 2+ alive worms");
    }
    let locals = local_ingame_teams(&worms)?;
    if locals.is_empty() {
        return Err("no local in-game team");
    }
    let (team, worm_idx) = current_turn_ids()?;
    if !locals.contains(&team) {
        return Err("not local turn");
    }
    let ours = worms
        .iter()
        .cloned()
        .find(|w| w.team == team && w.worm_idx == worm_idx)
        .or_else(|| worms.iter().cloned().find(|w| w.team == team))
        .ok_or("active worm not in alive list")?;
    let mut enemies: Vec<WormInfo> = worms
        .into_iter()
        .filter(|w| !locals.contains(&w.team) && w.hp > 0)
        .collect();
    if enemies.is_empty() {
        return Err("no enemy worm");
    }
    enemies.sort_by(|a, b| {
        let da = (a.x - ours.x).hypot(a.y - ours.y);
        let db = (b.x - ours.x).hypot(b.y - ours.y);
        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok((ours, enemies))
}

/// Active local turn worm + closest living enemy worm (any worm on opposing teams).
pub fn duel_pair() -> Result<(WormInfo, WormInfo), &'static str> {
    let (ours, enemies) = duel_context()?;
    Ok((ours, enemies[0].clone()))
}

/// Closest group of ≥2 living enemy worms within `max_dist_px` of each other.
/// Returns centroid (x, y) and member count.
pub fn enemy_cluster_centroid(max_dist_px: f64) -> Result<Option<(f64, f64, usize)>, &'static str> {
    let worms = list_alive_worms()?;
    let locals = local_ingame_teams(&worms)?;
    if locals.is_empty() {
        return Err("no local in-game team");
    }
    let enemies: Vec<WormInfo> = worms
        .into_iter()
        .filter(|w| !locals.contains(&w.team) && w.hp > 0)
        .collect();
    if enemies.len() < 2 {
        return Ok(None);
    }

    let mut best: Vec<WormInfo> = Vec::new();
    for seed in &enemies {
        let group: Vec<WormInfo> = enemies
            .iter()
            .cloned()
            .filter(|w| {
                let dx = w.x - seed.x;
                let dy = w.y - seed.y;
                dx * dx + dy * dy <= max_dist_px * max_dist_px
            })
            .collect();
        if group.len() > best.len() {
            best = group;
        }
    }
    if best.len() < 2 {
        return Ok(None);
    }
    let n = best.len() as f64;
    let cx = best.iter().map(|w| w.x).sum::<f64>() / n;
    let cy = best.iter().map(|w| w.y).sum::<f64>() / n;
    Ok(Some((cx, cy, best.len())))
}

/// Sample as if Teleport cursor were on `target` (for solver/tele reuse).
pub fn sample_aimed_at(target_x: f64, target_y: f64) -> Result<WaAimSample, &'static str> {
    unsafe {
        let (worm, _world, team_idx, worm_idx) = resolve_active_worm()?;
        let turn_active = read_u32(worm + OFF_WORM_TURN_ACTIVE).unwrap_or(0);
        let show_cursor = read_u32(worm + OFF_WORM_SHOW_CURSOR).unwrap_or(0);
        let worm_x = fixed_to_f64(read_i32(worm + OFF_WORM_POS_X).ok_or("bad pos x")?);
        let worm_y = fixed_to_f64(read_i32(worm + OFF_WORM_POS_Y).ok_or("bad pos y")?);
        Ok(WaAimSample {
            worm_x,
            worm_y,
            cursor_x: target_x,
            cursor_y: target_y,
            rel_x: target_x - worm_x,
            rel_y: target_y - worm_y,
            show_cursor,
            turn_active,
            team_idx,
            worm_idx,
        })
    }
}

/// Ghidra VA of MissileEntity vtable (`PTR_LAB_00664438`).
const GHIDRA_MISSILE_VT: u32 = 0x00664438;
const OFF_ENTITY_POS_X: usize = 0x84;
const OFF_ENTITY_POS_Y: usize = 0x88;
/// WorldEntity world ptr (same layout as worm).
const OFF_ENTITY_WORLD: usize = 0x2c;
/// WorldEntity type ? MissileEntity__Constructor passes `0xf`.
const OFF_ENTITY_TYPE: usize = 0x30;
/// MissileEntity fuse counter (−0x14 / physics tick); stock uses 0x7fffffff for infinite.
const OFF_MISSILE_FUSE: usize = 0x37C;
/// Secondary fuse slot used when primary hits 0 (HandleMessage @ 0x0050B85E).
const OFF_MISSILE_FUSE2: usize = 0x380;
const OFF_MISSILE_VX: usize = 0x90;
const OFF_MISSILE_VY: usize = 0x94;
/// WorldEntity wind scale — stock grenade 0, bazooka ONE; CM+0x230 wind applies when non-zero.
const OFF_ENTITY_WIND_SCALE: usize = 0x5c;
const FIXED_ONE: i32 = 0x10000;
/// Default: 10% of stock bazooka wind (`entity+0x5c` fraction). Overridden from golf.toml.
static GOLF_WIND_SCALE: AtomicU32 = AtomicU32::new(6554);

pub fn set_golf_wind_scale(fraction: f64) {
    let frac = fraction.clamp(0.0, 1.0);
    let fixed = (frac * FIXED_ONE as f64).round() as u32;
    GOLF_WIND_SCALE.store(fixed, Ordering::Relaxed);
}

fn golf_wind_scale_fixed() -> i32 {
    GOLF_WIND_SCALE.load(Ordering::Relaxed) as i32
}
/// WorldEntity motion fields — all zero when idle (WorldEntity__IsMoving @ 0x004FB580).
const OFF_MISSILE_MOTION: [usize; 5] = [0x90, 0x94, 0xB0, 0xB4, 0xB8];
const FUSE_FROZEN: i32 = 0x7FFF_FFFF;
const ENTITY_TYPE_MISSILE: u32 = 0xf;
/// World ? CollisionManager (WorldEntity__Constructor uses world+0x528).
const OFF_WORLD_COLLISION_MGR: usize = 0x528;
/// Global WorldEntity DwordList at CM+0x18 (cap@+0, count@+8, data@+0xc).
const OFF_CM_ENTITY_LIST: usize = 0x18;
/// Per-type lists at CM+0x28 + type*0x10.
const OFF_CM_TYPE_LIST_BASE: usize = 0x28;
const OFF_DWORDLIST_COUNT: usize = 0x8;
const OFF_DWORDLIST_DATA: usize = 0xc;

#[derive(Clone, Copy, Debug)]
pub struct MissileSample {
    pub ptr: usize,
    pub x: f64,
    pub y: f64,
}

fn active_world_ptr() -> Option<usize> {
    // Prefer world from GameSession even when SharedData worm lookup fails —
    // maintain_golf_missiles / list_missiles must keep working mid-handoff.
    resolve_world().ok().or_else(|| {
        unsafe {
            let (_, world, _, _) = resolve_active_worm().ok()?;
            Some(world)
        }
    })
}

/// Stock grenade wind off (`+0x5c = 0`). Used at rest and as worker-thread safety net.
pub fn disable_missile_wind(ptr: usize) -> bool {
    set_missile_wind_scale(ptr, 0)
}

pub fn is_missile_ptr(ent: usize) -> bool {
    let vt = rb(GHIDRA_MISSILE_VT);
    unsafe { is_missile_entity(ent, vt) }
}

/// `entity+0x5c` fraction of CM+0x230 wind — stock `apply_projectile_gravity` path.
pub fn set_missile_wind_scale(ptr: usize, scale_fixed: i32) -> bool {
    let vt = rb(GHIDRA_MISSILE_VT);
    unsafe {
        if !is_missile_entity(ptr, vt) {
            return false;
        }
        write_i32_at(ptr + OFF_ENTITY_WIND_SCALE, scale_fixed)
    }
}

/// Airborne golf ball: stock wind at `golf.toml` fraction; idle → 0 (no ground slide).
pub fn sync_golf_missile_wind(ptr: usize) -> bool {
    if missile_is_idle(ptr) {
        return disable_missile_wind(ptr);
    }
    set_missile_wind_scale(ptr, golf_wind_scale_fixed())
}

/// Re-apply golf ball rules on every live missile (fuse + clear wind at rest).
/// Always freeze unless already detonating — tracked balls still need this; the
/// game-thread hook alone is not enough (fuse also ticks outside gravity).
pub fn maintain_golf_missiles() {
    for m in list_missiles() {
        if missile_is_idle(m.ptr) {
            disable_missile_wind(m.ptr);
        }
        // Never re-freeze a ball we already armed on the game thread (fuse == 0).
        freeze_missile_fuse_unless_detonating(m.ptr);
    }
}

unsafe fn read_dwordlist_ptrs(list_base: usize) -> Vec<usize> {
    let mut out = Vec::new();
    if list_base == 0 || !readable(list_base, 0x10) {
        return out;
    }
    let Some(count) = read_i32(list_base + OFF_DWORDLIST_COUNT) else {
        return out;
    };
    if count <= 0 || count > 2048 {
        return out;
    }
    let Some(data) = read_usize(list_base + OFF_DWORDLIST_DATA) else {
        return out;
    };
    if data == 0 || !readable(data, count as usize * 4) {
        return out;
    }
    for i in 0..count as usize {
        if let Some(p) = read_usize(data + i * 4) {
            if p != 0 {
                out.push(p);
            }
        }
    }
    out
}

fn is_missile_entity(ent: usize, vt: usize) -> bool {
    unsafe {
        if !readable(ent, 0x90) {
            return false;
        }
        if let Some(ty) = read_u32(ent + OFF_ENTITY_TYPE) {
            if ty == ENTITY_TYPE_MISSILE {
                return true;
            }
        }
        if let Some(ev) = read_usize(ent) {
            if ev == vt {
                return true;
            }
        }
        false
    }
}

/// Live missiles via CollisionManager typed list (type 0xf), else global CM list.
pub fn list_missiles() -> Vec<MissileSample> {
    let Some(world) = active_world_ptr() else {
        return Vec::new();
    };
    let vt = rb(GHIDRA_MISSILE_VT);
    let mut out = Vec::new();
    unsafe {
        let Some(cm) = read_usize(world + OFF_WORLD_COLLISION_MGR) else {
            return out;
        };
        if cm == 0 || !readable(cm, 0x200) {
            return out;
        }
        let typed = cm + OFF_CM_TYPE_LIST_BASE + (ENTITY_TYPE_MISSILE as usize) * 0x10;
        let mut ptrs = read_dwordlist_ptrs(typed);
        if ptrs.is_empty() {
            ptrs = read_dwordlist_ptrs(cm + OFF_CM_ENTITY_LIST);
        }
        for ent in ptrs {
            if !is_missile_entity(ent, vt) {
                continue;
            }
            let ew = read_usize(ent + OFF_ENTITY_WORLD).unwrap_or(0);
            if ew != 0 && ew != world {
                continue;
            }
            if let (Some(x), Some(y)) = (
                read_i32(ent + OFF_ENTITY_POS_X),
                read_i32(ent + OFF_ENTITY_POS_Y),
            ) {
                out.push(MissileSample {
                    ptr: ent,
                    x: fixed_to_f64(x),
                    y: fixed_to_f64(y),
                });
            }
        }
    }
    out
}

pub fn list_missile_ptrs() -> std::collections::HashSet<usize> {
    list_missiles().into_iter().map(|m| m.ptr).collect()
}

/// Read missile pos if still a live MissileEntity; None when freed / overwritten.
pub fn read_missile_pos(ptr: usize) -> Option<(f64, f64)> {
    let vt = rb(GHIDRA_MISSILE_VT);
    unsafe {
        if !is_missile_entity(ptr, vt) {
            return None;
        }
        let x = fixed_to_f64(read_i32(ptr + OFF_ENTITY_POS_X)?);
        let y = fixed_to_f64(read_i32(ptr + OFF_ENTITY_POS_Y)?);
        Some((x, y))
    }
}

pub fn write_i32_at(addr: usize, val: i32) -> bool {
    write_bytes_at(addr, &val.to_le_bytes())
}

/// Prevent stock fuse expiry — golf ball detonates on idle, not on timer.
pub fn freeze_missile_fuse(ptr: usize) -> bool {
    let vt = rb(GHIDRA_MISSILE_VT);
    unsafe {
        if !is_missile_entity(ptr, vt) {
            return false;
        }
        write_i32_at(ptr + OFF_MISSILE_FUSE, FUSE_FROZEN)
            && write_i32_at(ptr + OFF_MISSILE_FUSE2, FUSE_FROZEN)
    }
}

/// Keep fuse infinite unless stock is already counting down to detonate (fuse == 0).
pub fn freeze_missile_fuse_unless_detonating(ptr: usize) -> bool {
    let vt = rb(GHIDRA_MISSILE_VT);
    unsafe {
        if !is_missile_entity(ptr, vt) {
            return false;
        }
        let f1 = read_i32(ptr + OFF_MISSILE_FUSE).unwrap_or(FUSE_FROZEN);
        let f2 = read_i32(ptr + OFF_MISSILE_FUSE2).unwrap_or(FUSE_FROZEN);
        if f1 == 0 || f2 == 0 {
            return true;
        }
        if f1 == FUSE_FROZEN && f2 == FUSE_FROZEN {
            return true;
        }
        freeze_missile_fuse(ptr)
    }
}

/// Stock WorldEntity__IsMoving — all motion fields exactly zero.
pub fn missile_is_idle(ptr: usize) -> bool {
    let vt = rb(GHIDRA_MISSILE_VT);
    unsafe {
        if !is_missile_entity(ptr, vt) {
            return false;
        }
        OFF_MISSILE_MOTION
            .iter()
            .all(|off| read_i32(ptr + off).unwrap_or(1) == 0)
    }
}

pub fn read_missile_vel(ptr: usize) -> Option<(i32, i32)> {
    let vt = rb(GHIDRA_MISSILE_VT);
    unsafe {
        if !is_missile_entity(ptr, vt) {
            return None;
        }
        Some((
            read_i32(ptr + OFF_MISSILE_VX)?,
            read_i32(ptr + OFF_MISSILE_VY)?,
        ))
    }
}

/// Add horizontal impulse (16.16 px/tick) — same field as WorldEntity__AddImpulse +0x90.
pub fn add_missile_vx(ptr: usize, delta: i32) -> bool {
    if delta == 0 {
        return true;
    }
    let vt = rb(GHIDRA_MISSILE_VT);
    unsafe {
        if !is_missile_entity(ptr, vt) {
            return false;
        }
        let Some(vx) = read_i32(ptr + OFF_MISSILE_VX) else {
            return false;
        };
        write_i32_at(ptr + OFF_MISSILE_VX, vx.saturating_add(delta))
    }
}

/// Queue stock detonation on the game thread — zero both fuse counters (vtable +0x38 path).
pub fn arm_missile_detonate(ptr: usize) -> bool {
    let vt = rb(GHIDRA_MISSILE_VT);
    unsafe {
        if !is_missile_entity(ptr, vt) {
            return false;
        }
        write_i32_at(ptr + OFF_MISSILE_FUSE, 0)
            && write_i32_at(ptr + OFF_MISSILE_FUSE2, 0)
    }
}

/// Debug: how many entities CM currently tracks (typed missiles / global).
pub fn missile_scan_diag() -> String {
    let Some(world) = active_world_ptr() else {
        return "no-world".into();
    };
    unsafe {
        let Some(cm) = read_usize(world + OFF_WORLD_COLLISION_MGR) else {
            return "no-cm".into();
        };
        let typed = cm + OFF_CM_TYPE_LIST_BASE + (ENTITY_TYPE_MISSILE as usize) * 0x10;
        let tcount = read_i32(typed + OFF_DWORDLIST_COUNT).unwrap_or(-1);
        let gcount = read_i32(cm + OFF_CM_ENTITY_LIST + OFF_DWORDLIST_COUNT).unwrap_or(-1);
        let n = list_missiles().len();
        format!("cm=0x{cm:08X} typed0xf={tcount} global={gcount} missiles={n}")
    }
}
