// logger.rs — tiny append-only file logger (GUI builds have no console).
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn init(path: &Path) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // Truncate if it grows past 2 MB so it never balloons.
    if std::fs::metadata(path).map(|m| m.len() > 2_000_000).unwrap_or(false) {
        let _ = std::fs::remove_file(path);
    }
    let _ = LOG_PATH.set(path.to_path_buf());
}

pub fn log(msg: &str) {
    let line = format!(
        "[{}] {}\n",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0),
        msg
    );
    eprint!("{}", line);
    if let Some(p) = LOG_PATH.get() {
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(p) {
            let _ = f.write_all(line.as_bytes());
        }
    }
}

#[macro_export]
macro_rules! logf {
    ($($arg:tt)*) => { $crate::logger::log(&format!($($arg)*)) };
}
