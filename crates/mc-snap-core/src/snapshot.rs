use crate::paths::ProjectLayout;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMeta {
    pub id: String,
    pub created_at: String,
    pub kind: String,
    pub from_minecraft: Option<String>,
    pub to_minecraft: Option<String>,
    #[serde(default)]
    pub skipped_mods: Vec<String>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub dir: PathBuf,
    pub meta: SnapshotMeta,
}

pub fn snapshots_dir(layout: &ProjectLayout) -> PathBuf {
    layout.snap_dir().join("snapshots")
}

pub fn create(layout: &ProjectLayout, meta: SnapshotMeta) -> Result<Snapshot> {
    let base = snapshots_dir(layout);
    std::fs::create_dir_all(&base)?;
    let dir = base.join(&meta.id);
    if dir.exists() {
        anyhow::bail!("snapshot id already exists: {}", meta.id);
    }
    std::fs::create_dir_all(&dir)?;

    let yml = layout.yml();
    if yml.is_file() {
        std::fs::copy(&yml, dir.join("mc-snap.yml"))
            .with_context(|| format!("copying {}", yml.display()))?;
    }
    let lock = layout.lock();
    if lock.is_file() {
        std::fs::copy(&lock, dir.join("mc-snap.lock"))
            .with_context(|| format!("copying {}", lock.display()))?;
    }
    let configs = layout.configs_dir();
    if configs.is_dir() {
        copy_dir_recursive(&configs, &dir.join("configs"))?;
    }

    let meta_path = dir.join("meta.toml");
    std::fs::write(&meta_path, toml::to_string_pretty(&meta)?)?;

    Ok(Snapshot { dir, meta })
}

pub fn list(layout: &ProjectLayout) -> Result<Vec<Snapshot>> {
    let base = snapshots_dir(layout);
    if !base.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&base)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let meta_path = path.join("meta.toml");
        if !meta_path.is_file() {
            continue;
        }
        let s = std::fs::read_to_string(&meta_path)?;
        let meta: SnapshotMeta = toml::from_str(&s)
            .with_context(|| format!("parsing {}", meta_path.display()))?;
        out.push(Snapshot { dir: path, meta });
    }
    out.sort_by(|a, b| a.meta.id.cmp(&b.meta.id));
    Ok(out)
}

pub fn load(layout: &ProjectLayout, id: &str) -> Result<Snapshot> {
    let dir = snapshots_dir(layout).join(id);
    let meta_path = dir.join("meta.toml");
    if !meta_path.is_file() {
        anyhow::bail!("no snapshot named {id}");
    }
    let s = std::fs::read_to_string(&meta_path)?;
    let meta: SnapshotMeta = toml::from_str(&s)?;
    Ok(Snapshot { dir, meta })
}

pub fn latest(layout: &ProjectLayout) -> Result<Option<Snapshot>> {
    Ok(list(layout)?.into_iter().next_back())
}

/// Restore yml/lock/configs from a snapshot back into the project root.
/// Existing files are overwritten. configs/ is fully replaced.
pub fn restore(layout: &ProjectLayout, snap: &Snapshot) -> Result<()> {
    let yml = snap.dir.join("mc-snap.yml");
    if yml.is_file() {
        std::fs::copy(&yml, layout.yml())?;
    }
    let lock = snap.dir.join("mc-snap.lock");
    if lock.is_file() {
        std::fs::copy(&lock, layout.lock())?;
    } else if layout.lock().exists() {
        std::fs::remove_file(layout.lock())?;
    }
    let cfg_src = snap.dir.join("configs");
    let cfg_dst = layout.configs_dir();
    if cfg_dst.is_dir() {
        std::fs::remove_dir_all(&cfg_dst)?;
    }
    if cfg_src.is_dir() {
        copy_dir_recursive(&cfg_src, &cfg_dst)?;
    }
    Ok(())
}

pub fn new_id() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("snap-{secs}")
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn layout_in(root: &Path) -> ProjectLayout {
        ProjectLayout::at(root.to_path_buf())
    }

    fn meta(id: &str) -> SnapshotMeta {
        SnapshotMeta {
            id: id.into(),
            created_at: "now".into(),
            kind: "update".into(),
            from_minecraft: Some("26.1.2".into()),
            to_minecraft: Some("26.1.3".into()),
            skipped_mods: vec![],
            note: None,
        }
    }

    fn write_project(layout: &ProjectLayout, yml: &str, cfg: &str) {
        std::fs::write(layout.yml(), yml).unwrap();
        std::fs::write(layout.lock(), "schema = 1\n").unwrap();
        std::fs::create_dir_all(layout.configs_dir()).unwrap();
        std::fs::write(layout.configs_dir().join("a.txt"), cfg).unwrap();
    }

    #[test]
    fn snapshot_roundtrip() {
        let td = TempDir::new().unwrap();
        let layout = layout_in(td.path());
        write_project(&layout, "schema: 1\n", "hello");

        let snap = create(&layout, meta("snap-1")).unwrap();
        assert!(snap.dir.join("mc-snap.yml").is_file());
        assert!(snap.dir.join("configs/a.txt").is_file());

        std::fs::write(layout.yml(), "schema: 2\n").unwrap();
        std::fs::write(layout.configs_dir().join("a.txt"), "changed").unwrap();

        let loaded = load(&layout, "snap-1").unwrap();
        restore(&layout, &loaded).unwrap();
        assert_eq!(std::fs::read_to_string(layout.yml()).unwrap(), "schema: 1\n");
        assert_eq!(
            std::fs::read_to_string(layout.configs_dir().join("a.txt")).unwrap(),
            "hello"
        );
    }

    #[test]
    fn list_returns_snapshots_in_id_order_and_latest_is_last() {
        let td = TempDir::new().unwrap();
        let layout = layout_in(td.path());
        write_project(&layout, "schema: 1\n", "x");
        create(&layout, meta("snap-1")).unwrap();
        create(&layout, meta("snap-3")).unwrap();
        create(&layout, meta("snap-2")).unwrap();

        let all = list(&layout).unwrap();
        let ids: Vec<&str> = all.iter().map(|s| s.meta.id.as_str()).collect();
        assert_eq!(ids, vec!["snap-1", "snap-2", "snap-3"]);

        let last = latest(&layout).unwrap().expect("a snapshot exists");
        assert_eq!(last.meta.id, "snap-3");
    }

    #[test]
    fn latest_is_none_when_empty() {
        let td = TempDir::new().unwrap();
        let layout = layout_in(td.path());
        assert!(latest(&layout).unwrap().is_none());
        assert!(list(&layout).unwrap().is_empty());
    }

    #[test]
    fn load_errors_on_unknown_id() {
        let td = TempDir::new().unwrap();
        let layout = layout_in(td.path());
        let err = load(&layout, "missing").unwrap_err();
        assert!(err.to_string().contains("no snapshot"));
    }

    #[test]
    fn create_rejects_duplicate_id() {
        let td = TempDir::new().unwrap();
        let layout = layout_in(td.path());
        write_project(&layout, "schema: 1\n", "x");
        create(&layout, meta("dup")).unwrap();
        let err = create(&layout, meta("dup")).unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn restore_replaces_configs_dir_entirely() {
        let td = TempDir::new().unwrap();
        let layout = layout_in(td.path());
        write_project(&layout, "schema: 1\n", "in-snap");
        create(&layout, meta("snap-1")).unwrap();

        // Add a file to configs/ that was NOT in the snapshot. After restore it should be gone.
        std::fs::write(layout.configs_dir().join("extra.txt"), "rogue").unwrap();

        let loaded = load(&layout, "snap-1").unwrap();
        restore(&layout, &loaded).unwrap();
        assert!(layout.configs_dir().join("a.txt").exists());
        assert!(
            !layout.configs_dir().join("extra.txt").exists(),
            "configs/ should be fully replaced"
        );
    }

    #[test]
    fn restore_removes_lock_when_snapshot_had_none() {
        let td = TempDir::new().unwrap();
        let layout = layout_in(td.path());
        // Project with no lockfile at snapshot time.
        std::fs::write(layout.yml(), "schema: 1\n").unwrap();
        std::fs::create_dir_all(layout.configs_dir()).unwrap();
        create(&layout, meta("snap-1")).unwrap();

        // User later runs install and a lock appears.
        std::fs::write(layout.lock(), "schema = 1\n").unwrap();

        let loaded = load(&layout, "snap-1").unwrap();
        restore(&layout, &loaded).unwrap();
        assert!(
            !layout.lock().exists(),
            "lock should be removed when snapshot had none"
        );
    }

    #[test]
    fn new_id_has_expected_prefix() {
        assert!(new_id().starts_with("snap-"));
    }
}
