//! Cross-process advisory lock on a project. Prevents two concurrent
//! `mc-snap install` / `update` / `start` invocations from stomping each
//! other's yml, lockfile, or `.mc-snap/server/`.
//!
//! Note: not to be confused with [`crate::lock`], which is the *lockfile schema*
//! (mc-snap.lock).

use anyhow::{Context, Result};
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::path::Path;

/// RAII guard. Drop releases the lock and removes the file on a best-effort basis.
pub struct ProjectLock {
    file: File,
}

impl ProjectLock {
    /// Acquire an exclusive advisory lock on `path`. Errors immediately if another
    /// process holds it — callers should surface a clear message to the user.
    pub fn acquire(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .with_context(|| format!("opening lock file {}", path.display()))?;
        file.try_lock_exclusive().with_context(|| {
            format!(
                "another mc-snap command holds the project lock at {}",
                path.display()
            )
        })?;
        Ok(Self { file })
    }
}

impl Drop for ProjectLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}
