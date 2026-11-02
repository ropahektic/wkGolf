use std::path::PathBuf;

use windows_sys::Win32::System::LibraryLoader::GetModuleFileNameW;

pub fn dll_dir(module: usize) -> PathBuf {
    if let Ok(home) = std::env::var("WKGOLF_HOME") {
        if !home.is_empty() {
            return PathBuf::from(home);
        }
    }
    let mut buf = [0u16; 512];
    let n = unsafe { GetModuleFileNameW(module as _, buf.as_mut_ptr(), buf.len() as u32) };
    if n == 0 || n as usize >= buf.len() {
        return PathBuf::from(".");
    }
    let s = String::from_utf16_lossy(&buf[..n as usize]);
    PathBuf::from(s)
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}
