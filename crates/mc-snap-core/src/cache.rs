use anyhow::Context;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

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
        std::fs::write(&p, bytes)?;
        Ok(p)
    }

    pub fn link_into(&self, sha256: &str, dst: &Path) -> anyhow::Result<()> {
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
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&src, dst)
                .with_context(|| format!("symlink {} -> {}", dst.display(), src.display()))?;
        }
        #[cfg(windows)]
        {
            std::fs::hard_link(&src, dst)
                .with_context(|| format!("hardlink {} -> {}", dst.display(), src.display()))?;
        }
        Ok(())
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}
