//! Minimal append-only log at `%TEMP%\game-maps-agent.log` for lifecycle events and
//! errors, so problems are diagnosable even when no app is connected. Kept small:
//! the file is truncated when it grows past 256 KB.

use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_BYTES: u64 = 256 * 1024;

fn path() -> PathBuf {
    std::env::temp_dir().join("game-maps-agent.log")
}

pub fn write(message: &str) {
    let path = path();
    let truncate = std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_BYTES);
    let file = std::fs::OpenOptions::new().create(true).append(!truncate).write(true).truncate(truncate).open(&path);
    if let Ok(mut f) = file {
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let _ = writeln!(f, "[{secs}] [pid {}] {message}", std::process::id());
    }
}
