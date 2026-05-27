use anyhow::Context;
use directories::ProjectDirs;
use std::path::{Path, PathBuf};

use crate::yml::Snap;

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
    /// Resolve the server materialization directory from the optional
    /// `server.location` field. `None`, empty, or "." means surface-level
    /// (project root, alongside mc-snap.yml).
    pub fn server_dir(&self, location: Option<&str>) -> PathBuf {
        match location.map(str::trim) {
            None | Some("") | Some(".") => self.root.clone(),
            Some(sub) => self.root.join(sub),
        }
    }

    pub fn server_dir_for(&self, snap: &Snap) -> PathBuf {
        self.server_dir(snap.server.location.as_deref())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_dir_none_is_root() {
        let layout = ProjectLayout::at(PathBuf::from("/srv/mc"));
        assert_eq!(layout.server_dir(None), PathBuf::from("/srv/mc"));
    }

    #[test]
    fn server_dir_dot_is_root() {
        let layout = ProjectLayout::at(PathBuf::from("/srv/mc"));
        assert_eq!(layout.server_dir(Some(".")), PathBuf::from("/srv/mc"));
    }

    #[test]
    fn server_dir_empty_is_root() {
        let layout = ProjectLayout::at(PathBuf::from("/srv/mc"));
        assert_eq!(layout.server_dir(Some("")), PathBuf::from("/srv/mc"));
    }

    #[test]
    fn server_dir_subdir_joins_root() {
        let layout = ProjectLayout::at(PathBuf::from("/srv/mc"));
        assert_eq!(
            layout.server_dir(Some("server")),
            PathBuf::from("/srv/mc/server")
        );
    }
}
