//! Frame HUD hook — after stock network-wait textbox (same GameRender tail as Hole/Par text).

use std::sync::atomic::{AtomicBool, Ordering};

use windows_sys::Win32::System::Memory::{VirtualProtect, PAGE_EXECUTE_READWRITE};

use crate::draw;
use crate::log;
use crate::wa_mem;

const GHIDRA_RENDER_WAIT_CALL: u32 = 0x0053_448B;
const GHIDRA_RENDER_WAIT_FN: u32 = 0x0053_4F20;
const PATCH_LEN: usize = 5;
const EXPECTED_CALL: [u8; 5] = [0xE8, 0x90, 0x0A, 0x00, 0x00];

static HOOKED: AtomicBool = AtomicBool::new(false);
static INSTALL_TRIED: AtomicBool = AtomicBool::new(false);
static HUD_ENABLED: AtomicBool = AtomicBool::new(true);

static mut ORIG_CALL_TARGET: usize = 0;

pub fn set_hud_enabled(on: bool) {
    HUD_ENABLED.store(on, Ordering::SeqCst);
    if !on {
        log::line("[wkGolf] hud = false in golf.toml — no render hook");
    }
}

pub fn install_hook_when_ready() -> bool {
    if !HUD_ENABLED.load(Ordering::SeqCst) {
        return false;
    }
    if HOOKED.load(Ordering::SeqCst) {
        return true;
    }
    if INSTALL_TRIED.swap(true, Ordering::SeqCst) {
        return HOOKED.load(Ordering::SeqCst);
    }
    if !wa_mem::in_game_session() || !wa_mem::headful_frame_active() {
        INSTALL_TRIED.store(false, Ordering::SeqCst);
        return false;
    }
    install_hook()
}

fn install_hook() -> bool {
    let site = wa_mem::ghidra_va(GHIDRA_RENDER_WAIT_CALL);
    let orig_fn = wa_mem::ghidra_va(GHIDRA_RENDER_WAIT_FN);

    unsafe {
        let mut orig_bytes = [0u8; PATCH_LEN];
        for i in 0..PATCH_LEN {
            orig_bytes[i] = std::ptr::read((site + i) as *const u8);
        }
        if orig_bytes != EXPECTED_CALL {
            log::line(&format!(
                "[wkGolf] hook: unexpected bytes @ 0x{GHIDRA_RENDER_WAIT_CALL:08X} \
                 ({orig_bytes:02X?}) — skip HUD (WA 3.8.1 may differ from RE build)"
            ));
            return false;
        }

        ORIG_CALL_TARGET = orig_fn;

        let hook = hooked_render_waiting as *const () as usize;
        let rel = (hook as isize - (site as isize + 5)) as i32;
        let mut patch = [0u8; PATCH_LEN];
        patch[0] = 0xE8;
        patch[1..5].copy_from_slice(&rel.to_le_bytes());

        let mut old = 0u32;
        if VirtualProtect(site as *const _, PATCH_LEN, PAGE_EXECUTE_READWRITE, &mut old) == 0 {
            log::line("[wkGolf] hook: VirtualProtect failed");
            return false;
        }
        std::ptr::copy_nonoverlapping(patch.as_ptr(), site as *mut u8, PATCH_LEN);
        let mut ignore = 0u32;
        let _ = VirtualProtect(site as *const _, PATCH_LEN, old, &mut ignore);
    }

    HOOKED.store(true, Ordering::SeqCst);
    log::line(&format!(
        "[wkGolf] HUD hook @ 0x{GHIDRA_RENDER_WAIT_CALL:08X} tail=0x{orig_fn:08X} (stock first)"
    ));
    true
}

/// Replaces stock `CALL RenderWaitingForPeersTextbox`.
unsafe extern "C" fn hooked_render_waiting() {
    let runtime: usize;
    core::arch::asm!("mov {}, edi", out(reg) runtime);

    let orig = ORIG_CALL_TARGET;
    core::arch::asm!(
        "mov eax, {runtime}",
        "call ecx",
        runtime = in(reg) runtime,
        in("ecx") orig,
        options(nostack),
    );

    if wa_mem::in_game_session() && wa_mem::headful_frame_active() {
        draw::flush_hud_overlay();
    }
}
