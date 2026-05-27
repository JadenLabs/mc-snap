use anyhow::Context;
use directories::ProjectDirs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ProjectLayout {
    pub root: PathBuf,
}

impl ProjectLayout {
    pub fn discover(start: &Path) -> anyhow::Result<Self> {
        let mut cur = start.canonicalize().unwrap_or_else(|_| start.to_path_buf());
        loop {
            if cur.join("mc-snap.yml").is_file() {
                return Ok(Self { root: cur });
            }
            match cur.parent() {
                Some(p) => cur = p.to_path_buf(),
                None => anyhow::bail!("no mc-snap.yml found in current directory or any parent"),
            }
        }
    }

    pub fn at(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn yml(&self) -> PathBuf {
        self.root.join("mc-snap.yml")
    }
    pub fn lock(&self) -> PathBuf {
        self.root.join("mc-snap.lock")
    }
    pub fn configs_dir(&self) -> PathBuf {
        self.root.join("configs")
    }
    pub fn snap_dir(&self) -> PathBuf {
        self.root.join(".mc-snap")
    }
    pub fn server_dir(&self) -> PathBuf {
        self.snap_dir().join("server")
    }
    pub fn state_file(&self) -> PathBuf {
        self.snap_dir().join("state.json")
    }
    pub fn pid_file(&self) -> PathBuf {
        self.snap_dir().join("pid")
    }
    pub fn rcon_secret(&self) -> PathBuf {
        self.snap_dir().join("rcon.secret")
    }
    pub fn lock_file(&self) -> PathBuf {
        self.snap_dir().join(".lock")
    }
}

#[derive(Debug, Clone)]
pub struct GlobalDirs {
    pub cache: PathBuf,
    pub jdks: PathBuf,
}

impl GlobalDirs {
    pub fn resolve() -> anyhow::Result<Self> {
        let pd = ProjectDirs::from("io", "mc-snap", "mc-snap")
            .context("could not resolve user directories")?;
        let base = pd.data_dir().to_path_buf();
        Ok(Self {
            cache: base.join("cache"),
            jdks: base.join("jdks"),
        })
    }

    pub fn ensure(&self) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.cache)?;
        std::fs::create_dir_all(&self.jdks)?;
        Ok(())
    }
}
