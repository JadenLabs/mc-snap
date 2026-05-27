use crate::compat::{self, CompatStatus};
use crate::orchestrate;
use anyhow::{Context, Result};
use mc_snap_core::lock::Lock;
use mc_snap_core::paths::ProjectLayout;
use mc_snap_core::snapshot::{self, SnapshotMeta};
use mc_snap_core::yml::{ModEntry, Snap};
use mc_snap_runtime::process;
use std::collections::HashSet;
use std::io::{BufRead, Write};

pub async fn run(
    target_mc: &str,
    skip_missing: bool,
    assume_yes: bool,
    loader_version: Option<String>,
) -> Result<()> {
    let layout = ProjectLayout::discover(&std::env::current_dir()?)?;
    let snap = Snap::from_path(&layout.yml())?;

    if let Some(pid) = process::read_pid(&layout.pid_file()) {
        if process::is_running(pid) {
            anyhow::bail!("server is running (pid {pid}); stop it with `mc-snap stop` before updating");
        }
    }

    let current_mc = snap.server.minecraft.clone();
    if current_mc == target_mc {
        println!("already at minecraft {target_mc}; nothing to update");
        return Ok(());
    }
    println!("checking mod compatibility: {current_mc} -> {target_mc}");

    let reports = compat::check_against(&snap, target_mc).await?;
    let mut skipped: Vec<String> = Vec::new();
    let mut suggested: std::collections::HashMap<String, String> = Default::default();
    let mut errored: Vec<(String, String)> = Vec::new();

    for r in &reports {
        match &r.status {
            CompatStatus::Compatible { suggested_version } => {
                suggested.insert(r.mod_id.clone(), suggested_version.clone());
                println!("  ok    {} -> {}", r.mod_id, suggested_version);
            }
            CompatStatus::Incompatible => {
                println!("  miss  {} (no version for {target_mc})", r.mod_id);
                skipped.push(r.mod_id.clone());
            }
            CompatStatus::Unknown => {
                println!("  url   {} (url-pinned; left as-is)", r.mod_id);
            }
            CompatStatus::Error(e) => {
                println!("  err   {} ({e})", r.mod_id);
                errored.push((r.mod_id.clone(), e.clone()));
            }
        }
    }

    if !errored.is_empty() {
        anyhow::bail!(
            "provider errors prevented compatibility check; aborting before any destructive action"
        );
    }

    if !skipped.is_empty() {
        if skip_missing {
            println!("--skip-missing: dropping {} incompatible mod(s)", skipped.len());
        } else if assume_yes {
            anyhow::bail!(
                "{} mod(s) lack a {target_mc} version: {}. Re-run with --skip-missing to drop them.",
                skipped.len(),
                skipped.join(", ")
            );
        } else {
            print!(
                "\n{} mod(s) have no version for {target_mc}: {}\nSkip these and continue? [y/N] ",
                skipped.len(),
                skipped.join(", ")
            );
            std::io::stdout().flush().ok();
            let mut line = String::new();
            std::io::stdin().lock().read_line(&mut line)?;
            let ans = line.trim().to_lowercase();
            if ans != "y" && ans != "yes" {
                println!("cancelled");
                return Ok(());
            }
        }
    }

    // Build the new yml in memory: bump MC, optionally bump loader, drop skipped mods,
    // and pin each surviving registry mod to its suggested version. URL mods pass through.
    let mut new_snap = snap.clone();
    new_snap.server.minecraft = target_mc.to_string();
    if let Some(lv) = loader_version {
        new_snap.server.loader.version = Some(lv);
    } else {
        // Loader version is tied to MC version; clear it so the loader resolves latest stable.
        new_snap.server.loader.version = None;
        new_snap.server.loader.installer = None;
    }
    let skip_set: HashSet<&str> = skipped.iter().map(|s| s.as_str()).collect();
    new_snap.mods.retain(|m| match m {
        ModEntry::Registry { id, .. } => !skip_set.contains(id.as_str()),
        ModEntry::Url { .. } => true,
    });
    for m in &mut new_snap.mods {
        if let ModEntry::Registry { id, version, .. } = m {
            if let Some(v) = suggested.get(id) {
                *version = v.clone();
            }
        }
    }

    // Resolve the full new lockfile up front — this hits the network for the loader
    // jar and every mod, populating the cache. If anything blows up, we have NOT
    // touched the user's yml/lock/server yet.
    println!("resolving new lockfile...");
    let new_lock = orchestrate::resolve(&new_snap).await?;

    // Take a snapshot of the current state before any destructive write.
    let meta = SnapshotMeta {
        id: snapshot::new_id(),
        created_at: now_rfc3339(),
        kind: "update".into(),
        from_minecraft: Some(current_mc.clone()),
        to_minecraft: Some(target_mc.to_string()),
        skipped_mods: skipped.clone(),
        note: None,
    };
    let snap_dir = snapshot::create(&layout, meta.clone())?;
    println!("snapshot saved: {}", snap_dir.meta.id);

    // Persist the new yml + lock.
    let new_yml_text = serde_yml::to_string(&new_snap)
        .context("serializing updated mc-snap.yml")?;
    std::fs::write(layout.yml(), new_yml_text)?;
    new_lock.write(&layout.lock())?;

    // Clean stale mods + launch jar, then materialize.
    if layout.server_dir().is_dir() {
        clean_with_old_lock_aware(&layout, &new_lock)?;
    }
    orchestrate::materialize(&layout, &new_snap, &new_lock).await?;

    println!(
        "updated {} from {} to {} ({} mods, {} skipped). revert with `mc-snap revert {}`",
        new_snap.server.name,
        current_mc,
        target_mc,
        new_snap.mods.len(),
        skipped.len(),
        snap_dir.meta.id
    );
    Ok(())
}

fn clean_with_old_lock_aware(layout: &ProjectLayout, new_lock: &Lock) -> Result<()> {
    orchestrate::clean_stale_artifacts(layout, new_lock)
}

fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("@{secs}")
}
