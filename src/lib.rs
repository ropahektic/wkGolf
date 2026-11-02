//! wkGolf — WormKit mini-golf (grenade = ball, stock physics).

#![allow(non_snake_case)]

mod aim;
mod ballistics;
mod bounce_spin;
mod config;
mod draw;
mod game;
mod hook_game;
mod hook_missile;
mod hook_physics;
mod hook_spin;
mod log;
mod missile;
mod paths;
mod spin;
mod terrain;
mod wa_mem;
mod worm_move;

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

const DLL_PROCESS_ATTACH: u32 = 1;
const DLL_PROCESS_DETACH: u32 = 0;

static RUNNING: AtomicBool = AtomicBool::new(false);
static MODULE: AtomicUsize = AtomicUsize::new(0);

#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllMain(
    module: *mut c_void,
    reason: u32,
    _reserved: *mut c_void,
) -> i32 {
    match reason {
        DLL_PROCESS_ATTACH => {
            MODULE.store(module as usize, Ordering::SeqCst);
            if !RUNNING.swap(true, Ordering::SeqCst) {
                std::thread::spawn(worker);
            }
        }
        DLL_PROCESS_DETACH => {
            RUNNING.store(false, Ordering::SeqCst);
        }
        _ => {}
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn wkGolf_load() {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(worker);
}

fn worker() {
    let module = MODULE.load(Ordering::SeqCst);
    let dir = paths::dll_dir(module);
    log::set_path(dir.join("wkGolf.log"));
    log::line("[wkGolf] loaded - v0.10.22 fuse-frame-arm");
    log::line("[wkGolf] put golf.toml + map PNG next to wkGolf.dll");
    log::line("[wkGolf] spin ok; fuse freeze+idle-arm on MissileEntity FrameFinish");

    let course = config::load_beside_dll(module);
    wa_mem::set_golf_wind_scale(course.wind_scale);
    hook_game::set_hud_enabled(course.hud);
    let mut golf = game::GolfGame::new(course);

    while RUNNING.load(Ordering::SeqCst) {
        let _ = hook_game::install_hook_when_ready();
        hook_physics::install_when_ready();
        hook_spin::install_when_ready();
        hook_missile::install_when_ready();
        golf.tick();
        std::thread::sleep(Duration::from_millis(16));
    }
    log::line("[wkGolf] unloaded");
}
