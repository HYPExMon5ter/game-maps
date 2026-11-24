//! Writing files that must never be left half written.

use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// Writes `bytes` to `path`, whole or not at all: a crash or a full disk leaves what was
/// there before. The folder is made if it's missing.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // A name of its own: two writes of the same file at once mustn't share their half-written one.
    static WRITES: AtomicU64 = AtomicU64::new(0);
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(format!(".{}-{}.tmp", std::process::id(), WRITES.fetch_add(1, Ordering::Relaxed)));
    let partial = path.with_file_name(name);
    let written = std::fs::File::create_new(&partial).and_then(|mut file| {
        file.write_all(bytes)?;
        file.sync_all()
    });
    // Replaces the file in one step (on Windows too).
    let result = written.and_then(|()| std::fs::rename(&partial, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_a_file_and_leaves_nothing_behind() {
        let dir = std::env::temp_dir().join(format!("game-maps-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("nested").join("pins.json");

        write_atomic(&file, b"one").unwrap();
        write_atomic(&file, b"two").unwrap();

        assert_eq!(std::fs::read(&file).unwrap(), b"two");
        assert_eq!(std::fs::read_dir(file.parent().unwrap()).unwrap().count(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
