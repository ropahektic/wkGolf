//! Game-thread wind: stock `apply_projectile_gravity` with scaled `entity+0x5c`.

use std::sync::atomic::{AtomicBool, Ordering};

use windows_sys::Win32::System::Memory::{
    VirtualAlloc, VirtualProtect, MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READWRITE,
};

use crate::bounce_spin;
use crate::log;
use crate::wa_mem;

const GHIDRA_APPLY_PROJECTILE_GRAVITY: u32 = 0x004F_CB30;
const PATCH_LEN: usize = 6;
const STOLEN: [u8; 6] = [0x55, 0x8B, 0xEC, 0x83, 0xE4, 0xF8];

static HOOKED: AtomicBool = AtomicBool::new(false);
static INSTALL_TRIED: AtomicBool = AtomicBool::new(false);

type ApplyProjectileGravityFn = unsafe extern "thiscall" fn(*mut u8, *mut u8);

static mut ORIG: Option<ApplyProjectileGravityFn> = None;

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
    let site = wa_mem::ghidra_va(GHIDRA_APPLY_PROJECTILE_GRAVITY);
    unsafe {
        let mut cur = [0u8; PATCH_LEN];
        for i in 0..PATCH_LEN {
            cur[i] = std::ptr::read((site + i) as *const u8);
        }
        if cur != STOLEN {
            log::line(&format!(
                "[wkGolf] physics hook: unexpected bytes @ 0x{GHIDRA_APPLY_PROJECTILE_GRAVITY:08X} \
                 ({cur:02X?})"
            ));
            return false;
        }

        let tramp = build_trampoline(site);
        if tramp.is_null() {
            log::line("[wkGolf] physics hook: VirtualAlloc failed");
            return false;
        }
        ORIG = Some(std::mem::transmute(tramp));

        let hook = hooked_apply_projectile_gravity as *const () as usize;
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
        "[wkGolf] physics hook @ 0x{GHIDRA_APPLY_PROJECTILE_GRAVITY:08X} wind+bounce-spin"
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

unsafe extern "thiscall" fn hooked_apply_projectile_gravity(cm: *mut u8, entity: *mut u8) {
    let ent = entity as usize;
    if wa_mem::is_missile_ptr(ent) {
        wa_mem::sync_golf_missile_wind(ent);
    }
    if let Some(orig) = ORIG {
        orig(cm, entity);
    }
    if wa_mem::is_missile_ptr(ent) {
        bounce_spin::on_gravity_missile(ent);
    }
}
