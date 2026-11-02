//! Append-only log next to wkGolf.dll (WKGOLF_HOME) or %LOCALAPPDATA%\\OpenWA\\runtime.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

static LOG_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn set_path(path: PathBuf) {
    if let Ok(mut g) = LOG_PATH.lock() {
        *g = Some(path);
    }
}

fn default_runtime_log() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    Some(
        PathBuf::from(local)
            .join("OpenWA")
            .join("runtime")
            .join("wkGolf.log"),
    )
}

pub fn line(msg: &str) {
    let path = LOG_PATH
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .or_else(default_runtime_log);
    let Some(path) = path else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "{msg}");
        let _ = f.flush();
    }
}
