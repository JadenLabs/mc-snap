use anyhow::{Context, Result};
use inquire::{ui::RenderConfig, MultiSelect};
use std::collections::HashSet;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use crate::paths::ProjectLayout;
use crate::style;
use crate::yml::{FileRef, Snap};

/// A config file discovered under a server's `config/` directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigCandidate {
    /// Absolute path to the source file on disk.
    pub abs: PathBuf,
    /// Path relative to the server directory (e.g. `config/chunky.toml`).
    pub server_rel: String,
}

/// File extensions we treat as text-based mod configuration. Anything outside
/// this set (jars, archives, binary state, images, logs) is skipped so
/// `init --detect` and `config detect` don't sweep up artifacts that aren't
/// meant to be version-controlled config.
const CONFIG_EXTENSIONS: &[&str] = &[
    "toml",
    "json",
    "json5",
    "jsonc",
    "yaml",
    "yml",
    "properties",
    "conf",
    "cfg",
    "ini",
    "txt",
    "snbt",
    "hocon",
    "xml",
    "lang",
    "mcmeta",
];

/// Directory names under `config/` that hold generated or transient data rather
/// than editable config. Skipped wholesale so we don't track per-world caches,
/// rolling logs, or backups some mods drop next to their real config.
const SKIP_DIRS: &[&str] = &[
    "cache",
    "caches",
    "logs",
    "backup",
    "backups",
    "crash-reports",
];

/// Decide whether a path discovered under `config/` is worth tracking. Rejects
/// hidden files/dirs, known transient subdirectories, and anything whose
/// extension isn't a recognized config format (this is what filters out jars).
fn is_trackable_config(server_rel: &str) -> bool {
    let parts: Vec<&str> = server_rel.split('/').collect();
    for comp in &parts {
        if comp.starts_with('.') {
            return false;
        }
    }
    // Skip files living inside a transient directory (but not the filename itself).
    if let Some((_file, dirs)) = parts.split_last() {
        if dirs
            .iter()
            .any(|d| SKIP_DIRS.contains(&d.to_ascii_lowercase().as_str()))
        {
            return false;
        }
    }
    let Some(name) = parts.last() else {
        return false;
    };
    match name.rsplit_once('.') {
        Some((_, ext)) => CONFIG_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()),
        None => false,
    }
}

/// Walk the server's `config/` directory and return every config file found,
/// deepest last. `server_dir` is the directory that contains `config/`. Files
/// that aren't recognized config formats (jars, archives, binaries) and known
/// transient subdirectories are filtered out; see [`is_trackable_config`].
/// Symlinks are followed but cycles are not detected; standard Minecraft setups
/// don't have any.
pub fn scan_config_dir(server_dir: &Path) -> Result<Vec<ConfigCandidate>> {
    let config_dir = server_dir.join("config");
    if !config_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    walk(&config_dir, server_dir, &mut out)?;
    out.retain(|c| is_trackable_config(&c.server_rel));
    out.sort_by(|a, b| a.server_rel.cmp(&b.server_rel));
    Ok(out)
}

fn walk(dir: &Path, server_dir: &Path, out: &mut Vec<ConfigCandidate>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        if p.is_dir() {
            walk(&p, server_dir, out)?;
        } else if p.is_file() {
            let rel = p
                .strip_prefix(server_dir)
                .with_context(|| format!("path not under server dir: {}", p.display()))?;
            let server_rel = rel.to_string_lossy().replace('\\', "/");
            out.push(ConfigCandidate {
                abs: p.clone(),
                server_rel,
            });
        }
    }
    Ok(())
}

/// Filter out candidates whose `dst` already appears in `existing`.
pub fn untracked<'a>(
    candidates: &'a [ConfigCandidate],
    existing: &[FileRef],
) -> Vec<&'a ConfigCandidate> {
    let tracked: HashSet<&str> = existing.iter().map(|f| f.dst.as_str()).collect();
    candidates
        .iter()
        .filter(|c| !tracked.contains(c.server_rel.as_str()))
        .collect()
}

/// Copy `candidate` from the server directory into the project's `configs/`
/// directory, mirroring the relative path. Returns the FileRef that should be
/// added to `snap.config.files`.
pub fn track_candidate(project_root: &Path, candidate: &ConfigCandidate) -> Result<FileRef> {
    let configs_root = project_root.join("configs");
    let rel = candidate
        .server_rel
        .strip_prefix("config/")
        .unwrap_or(&candidate.server_rel);
    let dst_abs = configs_root.join(rel);
    if let Some(parent) = dst_abs.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::copy(&candidate.abs, &dst_abs).with_context(|| {
        format!(
            "copying {} -> {}",
            candidate.abs.display(),
            dst_abs.display()
        )
    })?;
    let src_rel = format!("configs/{rel}");
    Ok(FileRef {
        src: src_rel,
        dst: candidate.server_rel.clone(),
    })
}

pub async fn run_detect(all: bool) -> Result<()> {
    let layout = ProjectLayout::discover(&std::env::current_dir()?)?;
    let yml_path = layout.yml();
    let mut snap = Snap::from_path(&yml_path)?;
    let server_dir = layout.server_dir_for(&snap);

    let candidates = scan_config_dir(&server_dir)?;
    if candidates.is_empty() {
        println!(
            "no config/ directory at {} (run the server once to generate mod configs)",
            server_dir.join("config").display()
        );
        return Ok(());
    }

    let new_refs = untracked(&candidates, &snap.config.files);
    if new_refs.is_empty() {
        println!(
            "{} all {} config file(s) are already tracked",
            style::ok(),
            candidates.len()
        );
        return Ok(());
    }

    let interactive = !all && std::io::stdin().is_terminal();
    let selected: Vec<&ConfigCandidate> = if interactive {
        prompt_select(&new_refs)?
    } else {
        new_refs.to_vec()
    };

    if selected.is_empty() {
        println!("nothing selected; no changes written");
        return Ok(());
    }

    let mut added = Vec::with_capacity(selected.len());
    for c in &selected {
        let fr = track_candidate(&layout.root, c)?;
        added.push(fr);
    }

    snap.config.files.extend(added.iter().cloned());
    snap.validate()?;
    let body = super::init::render_yml(&snap);
    std::fs::write(&yml_path, body)?;

    println!();
    println!(
        "{} tracked {} config file(s); copied into ./configs/",
        style::ok(),
        added.len()
    );
    for f in &added {
        println!("  {} {} -> {}", style::dim("+"), f.dst, f.src);
    }
    Ok(())
}

fn prompt_select<'a>(candidates: &[&'a ConfigCandidate]) -> Result<Vec<&'a ConfigCandidate>> {
    inquire::set_global_render_config(RenderConfig::default());
    let labels: Vec<String> = candidates.iter().map(|c| c.server_rel.clone()).collect();
    let defaults: Vec<usize> = (0..labels.len()).collect();
    let answers = MultiSelect::new(
        "Track these config files? (space toggles, enter confirms)",
        labels.clone(),
    )
    .with_default(&defaults)
    .with_help_message(
        "everything is selected by default; deselect anything you don't want tracked",
    )
    .prompt()?;

    let answer_set: HashSet<&str> = answers.iter().map(|s| s.as_str()).collect();
    Ok(candidates
        .iter()
        .filter(|c| answer_set.contains(c.server_rel.as_str()))
        .copied()
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn touch(p: &Path, body: &str) {
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(p, body).unwrap();
    }

    #[test]
    fn scan_returns_empty_when_no_config_dir() {
        let td = TempDir::new().unwrap();
        let out = scan_config_dir(td.path()).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn scan_finds_flat_and_nested_files() {
        let td = TempDir::new().unwrap();
        touch(&td.path().join("config/chunky.toml"), "x=1");
        touch(&td.path().join("config/luckperms/config.yml"), "y: 2");
        touch(&td.path().join("config/sub/dir/file.json5"), "{}");
        let mut out = scan_config_dir(td.path()).unwrap();
        out.sort_by(|a, b| a.server_rel.cmp(&b.server_rel));
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].server_rel, "config/chunky.toml");
        assert_eq!(out[1].server_rel, "config/luckperms/config.yml");
        assert_eq!(out[2].server_rel, "config/sub/dir/file.json5");
    }

    #[test]
    fn scan_skips_jars_and_non_config_files() {
        let td = TempDir::new().unwrap();
        touch(&td.path().join("config/real.toml"), "x=1");
        touch(&td.path().join("config/bundled-mod.jar"), "MZ");
        touch(&td.path().join("config/world.dat"), "binary");
        touch(&td.path().join("config/icon.png"), "img");
        touch(&td.path().join("config/archive.zip"), "zip");
        let out = scan_config_dir(td.path()).unwrap();
        let rels: Vec<&str> = out.iter().map(|c| c.server_rel.as_str()).collect();
        assert_eq!(rels, vec!["config/real.toml"]);
    }

    #[test]
    fn scan_skips_transient_subdirs_and_hidden_files() {
        let td = TempDir::new().unwrap();
        touch(&td.path().join("config/keep.json"), "{}");
        touch(&td.path().join("config/cache/lookup.json"), "{}");
        touch(&td.path().join("config/logs/debug.txt"), "log");
        touch(&td.path().join("config/backups/old.toml"), "x=1");
        touch(&td.path().join("config/.hidden.toml"), "x=1");
        let out = scan_config_dir(td.path()).unwrap();
        let rels: Vec<&str> = out.iter().map(|c| c.server_rel.as_str()).collect();
        assert_eq!(rels, vec!["config/keep.json"]);
    }

    #[test]
    fn is_trackable_config_extension_allowlist() {
        assert!(is_trackable_config("config/chunky.toml"));
        assert!(is_trackable_config("config/pack.mcmeta"));
        assert!(is_trackable_config("config/Mod/settings.JSON"));
        assert!(!is_trackable_config("config/mod.jar"));
        assert!(!is_trackable_config("config/noext"));
        assert!(!is_trackable_config("config/cache/x.json"));
    }

    #[test]
    fn untracked_filters_existing() {
        let candidates = vec![
            ConfigCandidate {
                abs: PathBuf::from("/x/config/a.toml"),
                server_rel: "config/a.toml".into(),
            },
            ConfigCandidate {
                abs: PathBuf::from("/x/config/b.toml"),
                server_rel: "config/b.toml".into(),
            },
        ];
        let existing = vec![FileRef {
            src: "configs/a.toml".into(),
            dst: "config/a.toml".into(),
        }];
        let new = untracked(&candidates, &existing);
        assert_eq!(new.len(), 1);
        assert_eq!(new[0].server_rel, "config/b.toml");
    }

    #[test]
    fn track_candidate_copies_and_returns_fileref() {
        let td = TempDir::new().unwrap();
        let server_file = td.path().join("config/chunky.toml");
        touch(&server_file, "tasks = []\n");
        let candidate = ConfigCandidate {
            abs: server_file.clone(),
            server_rel: "config/chunky.toml".into(),
        };
        let fr = track_candidate(td.path(), &candidate).unwrap();
        assert_eq!(fr.src, "configs/chunky.toml");
        assert_eq!(fr.dst, "config/chunky.toml");
        let copied = td.path().join("configs/chunky.toml");
        assert!(copied.is_file());
        assert_eq!(std::fs::read_to_string(&copied).unwrap(), "tasks = []\n");
    }

    #[test]
    fn track_candidate_mirrors_nested_paths() {
        let td = TempDir::new().unwrap();
        let server_file = td.path().join("config/luckperms/inner/config.yml");
        touch(&server_file, "node: hi\n");
        let candidate = ConfigCandidate {
            abs: server_file,
            server_rel: "config/luckperms/inner/config.yml".into(),
        };
        let fr = track_candidate(td.path(), &candidate).unwrap();
        assert_eq!(fr.src, "configs/luckperms/inner/config.yml");
        assert_eq!(fr.dst, "config/luckperms/inner/config.yml");
        assert!(td
            .path()
            .join("configs/luckperms/inner/config.yml")
            .is_file());
    }
}
