//! Keyboard polling (GetAsyncKeyState) — rising-edge helpers.

use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

pub const VK_J: i32 = 0x4A;
pub const VK_K: i32 = 0x4B;
pub const VK_L: i32 = 0x4C;

pub fn key_down(vk: i32) -> bool {
    unsafe { GetAsyncKeyState(vk) as u16 & 0x8000 != 0 }
}

#[derive(Default)]
pub struct Edge {
    prev: bool,
}

impl Edge {
    pub fn rise(&mut self, down: bool) -> bool {
        let out = down && !self.prev;
        self.prev = down;
        out
    }
}
