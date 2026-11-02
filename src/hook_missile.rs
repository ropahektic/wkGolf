//! MissileEntity__HandleMessage — freeze fuse + idle-arm every FrameFinish (msg 2).
//!
//! Stock fuse ticks here (−0x14 / frame). Gravity-hook freeze alone is not enough.

use std::sync::atomic::{AtomicBool, Ordering};

use windows_sys::Win32::System::Memory::{
    VirtualAlloc, VirtualProtect, MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READWRITE,
};

use crate::bounce_spin;
use crate::log;
use crate::wa_mem;

const GHIDRA_MISSILE_HM: u32 = 0x0050_B400;
const PATCH_LEN: usize = 6;
const STOLEN: [u8; 6] = [0x55, 0x8B, 0xEC, 0x83, 0xE4, 0xF8];
const MSG_FRAME_FINISH: u32 = 2;

static HOOKED: AtomicBool = AtomicBool::new(false);
static INSTALL_TRIED: AtomicBool = AtomicBool::new(false);

type HandleMessageFn = unsafe extern "thiscall" fn(*mut u8, u32, u32, u32, *mut u8) -> u32;

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
    let site = wa_mem::ghidra_va(GHIDRA_MISSILE_HM);
    unsafe {
        let mut cur = [0u8; PATCH_LEN];
        for i in 0..PATCH_LEN {
            cur[i] = std::ptr::read((site + i) as *const u8);
        }
        if cur != STOLEN {
            log::line(&format!(
                "[wkGolf] missile HM hook: unexpected bytes @ 0x{GHIDRA_MISSILE_HM:08X} ({cur:02X?})"
            ));
            return false;
        }

        let tramp = build_trampoline(site);
        if tramp.is_null() {
            log::line("[wkGolf] missile HM hook: VirtualAlloc failed");
            return false;
        }
        ORIG = Some(std::mem::transmute(tramp));

        let hook = hooked_missile_handle_message as *const () as usize;
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
        "[wkGolf] missile HM hook @ 0x{GHIDRA_MISSILE_HM:08X} fuse-freeze+idle-arm FrameFinish"
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

unsafe extern "thiscall" fn hooked_missile_handle_message(
    this: *mut u8,
    a: u32,
    msg: u32,
    b: u32,
    c: *mut u8,
) -> u32 {
    let ent = this as usize;
    if msg == MSG_FRAME_FINISH {
        bounce_spin::pre_frame_missile(ent);
    }
    let ret = if let Some(orig) = ORIG {
        orig(this, a, msg, b, c)
    } else {
        0
    };
    if msg == MSG_FRAME_FINISH {
        bounce_spin::post_frame_missile(ent);
    }
    ret
}
