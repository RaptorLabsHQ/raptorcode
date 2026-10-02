//! Crash-safe file writes: content goes to a sibling temp file, is synced to
//! disk, then renamed over the target, so a reader never observes a torn file.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// Monotonic counter for unique temp-file names during atomic writes.
static ATOMIC_WRITE_NONCE: AtomicU64 = AtomicU64::new(0);

/// Atomically writes `contents` to `path`: content is written to a sibling temp
/// file, flushed to disk, then renamed over the target. A crash can leave the
/// temp file behind but never a torn target — a concurrent or post-crash reader
/// always sees either the old file or the fully-written new one. `fs::rename`
/// replaces an existing destination on both Unix and Windows, so this serves
/// both create-new (via [`atomic_create`]) and in-place update.
pub(crate) fn atomic_write(path: &Path, contents: &str) -> io::Result<()> {
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty());
    let dir = dir.unwrap_or_else(|| Path::new("."));
    let base = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "metadata".to_owned());
    let nonce = ATOMIC_WRITE_NONCE.fetch_add(1, Ordering::Relaxed);
    let tmp_path = dir.join(format!(".{base}.tmp.{}.{nonce}", std::process::id()));

    let write = || -> io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()
    };
    if let Err(error) = write() {
        let _ = fs::remove_file(&tmp_path);
        return Err(error);
    }
    if let Err(error) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(error);
    }
    Ok(())
}

/// Atomically creates `path`, failing with `AlreadyExists` if it is already
/// present. The check→write window is acceptable for content-addressed ids and,
/// unlike `create_new` + a separate write, never leaves a torn/empty target that
/// would permanently wedge the id after a crash.
pub(crate) fn atomic_create(path: &Path, contents: &str) -> io::Result<()> {
    if path.try_exists()? {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "metadata file already exists",
        ));
    }
    atomic_write(path, contents)
}
