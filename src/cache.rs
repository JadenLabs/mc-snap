use anyhow::Context;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// How cache artifacts are placed into the server directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LinkMode {
    /// Platform default: symlink on Unix, hardlink on Windows.
    #[default]
    Auto,
    /// Copy the file outright (no link back to the cache).
    Copy,
    /// Symlink on every platform.
    Symlink,
}

pub struct ContentCache {
    root: PathBuf,
}

impl ContentCache {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn path_for(&self, sha256: &str) -> PathBuf {
        let (a, b) = sha256.split_at(2);
        self.root.join(a).join(b)
    }

    pub fn contains(&self, sha256: &str) -> bool {
        self.path_for(sha256).is_file()
    }

    pub fn store(&self, sha256: &str, bytes: &[u8]) -> anyhow::Result<PathBuf> {
        let p = self.path_for(sha256);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Write to a sibling temp file then rename so an interrupted run can't leave a
        // truncated file at the canonical path.
        let tmp = p.with_extension(format!("tmp.{}", std::process::id()));
        std::fs::write(&tmp, bytes).with_context(|| format!("writing {}", tmp.display()))?;
        match std::fs::rename(&tmp, &p) {
            Ok(()) => Ok(p),
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                Err(e).with_context(|| format!("renaming into cache {}", p.display()))
            }
        }
    }

    pub fn link_into(&self, sha256: &str, dst: &Path, mode: LinkMode) -> anyhow::Result<()> {
        let src = self.path_for(sha256);
        if !src.is_file() {
            anyhow::bail!("cache miss for {sha256}");
        }
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if dst.exists() {
            std::fs::remove_file(dst).ok();
        }
        match mode {
            LinkMode::Copy => {
                std::fs::copy(&src, dst)
                    .with_context(|| format!("copy {} -> {}", src.display(), dst.display()))?;
            }
            LinkMode::Symlink => symlink_file(&src, dst)?,
            LinkMode::Auto => {
                #[cfg(unix)]
                symlink_file(&src, dst)?;
                #[cfg(windows)]
                std::fs::hard_link(&src, dst)
                    .with_context(|| format!("hardlink {} -> {}", dst.display(), src.display()))?;
            }
        }
        Ok(())
    }
}

fn symlink_file(src: &Path, dst: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    std::os::unix::fs::symlink(src, dst)
        .with_context(|| format!("symlink {} -> {}", dst.display(), src.display()))?;
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(src, dst).with_context(|| {
        format!(
            "symlink {} -> {} (Windows symlinks need Developer Mode or admin rights)",
            dst.display(),
            src.display()
        )
    })?;
    Ok(())
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}
