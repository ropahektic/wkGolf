//! Lockstep spin dial — Jump / JumpUp / StartTurn on WormEntity__HandleMessage.
//!
//! Enter → Jump (0x24) → step right; Backspace → JumpUp (0x25) → step left.
//! StartTurn (0x34) → NO SPIN.
//!
//! Jump/JumpUp are **swallowed** (orig not called) while the worm's turn is active so
//! stock jump/backflip state does not diverge. Both peers still see the same msgs and
//! step the dial identically.

use std::sync::atomic::{AtomicBool, Ordering};

use windows_sys::Win32::System::Memory::{
    VirtualAlloc, VirtualProtect, MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READWRITE,
};

use crate::log;
use crate::spin;
use crate::wa_mem;

const GHIDRA_HANDLE_MESSAGE: u32 = 0x0051_0B40;
const PATCH_LEN: usize = 6;
const STOLEN: [u8; 6] = [0x55, 0x8B, 0xEC, 0x83, 0xE4, 0xF8];

const MSG_JUMP: u32 = 0x24;
const MSG_JUMP_UP: u32 = 0x25;
const MSG_START_TURN: u32 = 0x34;

static HOOKED: AtomicBool = AtomicBool::new(false);
static INSTALL_TRIED: AtomicBool = AtomicBool::new(false);

type HandleMessageFn = unsafe extern "thiscall" fn(*mut u8, u32, u32, u32, *mut u8);

static mut ORIG: Option<HandleMessageFn> = None;

pub fn install_when_ready() -> bool {
    if HOOKED.load(Ordering::SeqCst) {
        return true;
    }
    if INSTALL_TRIED.swap(true, Ordering::SeqCst) {
        return HOOKED.load(Ordering::SeqCst);
    }
    if !wa_mem::in_game_session() {
        INSTALL_TRIED.store(false, Ordering::SeqCst);
        return false;
    }
    install_hook()
}

fn install_hook() -> bool {
    let site = wa_mem::ghidra_va(GHIDRA_HANDLE_MESSAGE);
    unsafe {
        let mut cur = [0u8; PATCH_LEN];
        for i in 0..PATCH_LEN {
            cur[i] = std::ptr::read((site + i) as *const u8);
        }
        if cur != STOLEN {
            log::line(&format!(
                "[wkGolf] spin hook: unexpected bytes @ 0x{GHIDRA_HANDLE_MESSAGE:08X} ({cur:02X?})"
            ));
            return false;
        }

        let tramp = build_trampoline(site);
        if tramp.is_null() {
            log::line("[wkGolf] spin hook: VirtualAlloc failed");
            return false;
        }
        ORIG = Some(std::mem::transmute(tramp));

        let hook = hooked_handle_message as *const () as usize;
        let rel = (hook as isize - (site as isize + 5)) as i32;
        let mut patch = [0x90u8; PATCH_LEN];
        patch[0] = 0xE9;
        patch[1..5].copy_from_slice(&rel.to_le_bytes());

        let mut old = 0u32;
        if VirtualProtect(site as *const _, PATCH_LEN, PAGE_EXECUTE_READWRITE, &mut old) == 0 {
            return false;
        }
        std::ptr::copy_nonoverlapping(patch.as_ptr(), site as *mut u8, PATCH_LEN);
        let mut ignore = 0u32;
        let _ = VirtualProtect(site as *const _, PATCH_LEN, old, &mut ignore);
    }

    HOOKED.store(true, Ordering::SeqCst);
    log::line(&format!(
        "[wkGolf] spin hook @ 0x{GHIDRA_HANDLE_MESSAGE:08X} dial swallows Jump/JumpUp"
    ));
    true
}

unsafe fn build_trampoline(site: usize) -> *const u8 {
    let total = STOLEN.len() + 5;
    let mem = VirtualAlloc(
        std::ptr::null_mut(),
        total,
        MEM_COMMIT | MEM_RESERVE,
        PAGE_EXECUTE_READWRITE,
    );
    if mem.is_null() {
        return std::ptr::null();
    }
    std::ptr::copy_nonoverlapping(STOLEN.as_ptr(), mem as *mut u8, STOLEN.len());
    let jmp_site = mem as usize + STOLEN.len();
    let target = site + STOLEN.len();
    let rel = (target as isize - (jmp_site as isize + 5)) as i32;
    let mut jmp = [0xE9u8, 0, 0, 0, 0];
    jmp[1..5].copy_from_slice(&rel.to_le_bytes());
    std::ptr::copy_nonoverlapping(jmp.as_ptr(), jmp_site as *mut u8, 5);
    mem as *const u8
}

unsafe extern "thiscall" fn hooked_handle_message(
    this: *mut u8,
    a: u32,
    msg: u32,
    b: u32,
    c: *mut u8,
) {
    let worm = this as usize;
    let swallow = should_swallow_jump(worm, msg);
    observe_spin_message(worm, msg);
    if swallow {
        return;
    }
    if let Some(orig) = ORIG {
        orig(this, a, msg, b, c);
    }
}

fn should_swallow_jump(worm: usize, msg: u32) -> bool {
    if msg != MSG_JUMP && msg != MSG_JUMP_UP {
        return false;
    }
    if worm == 0 || !wa_mem::in_game_session() {
        return false;
    }
    wa_mem::read_worm_turn_active(worm)
}

fn observe_spin_message(worm: usize, msg: u32) {
    if worm == 0 || !wa_mem::in_game_session() {
        return;
    }
    let team = match wa_mem::read_u32_at(worm + wa_mem::OFF_WORM_TEAM) {
        Some(t) => t,
        None => return,
    };

    match msg {
        MSG_START_TURN => {
            spin::reset_turn(team, "StartTurn");
        }
        MSG_JUMP | MSG_JUMP_UP => {
            if !wa_mem::read_worm_turn_active(worm) {
                return;
            }
            if msg == MSG_JUMP {
                let _ = spin::step_right(team);
            } else {
                let _ = spin::step_left(team);
            }
        }
        _ => {}
    }
}
