use crate::orchestrate;
use anyhow::Result;
use mc_snap_core::paths::ProjectLayout;
use mc_snap_core::snapshot;
use mc_snap_core::yml::Snap;
use mc_snap_runtime::process;

pub async fn run(id: Option<String>, list: bool) -> Result<()> {
    let layout = ProjectLayout::discover(&std::env::current_dir()?)?;

    if list {
        let all = snapshot::list(&layout)?;
        if all.is_empty() {
            println!("no snapshots");
            return Ok(());
        }
        println!("snapshots (oldest first):");
        for s in &all {
            let from = s.meta.from_minecraft.as_deref().unwrap_or("?");
            let to = s.meta.to_minecraft.as_deref().unwrap_or("?");
            let note = if s.meta.skipped_mods.is_empty() {
                String::new()
            } else {
                format!(" (skipped: {})", s.meta.skipped_mods.join(", "))
            };
            println!(
                "  {}  {} {} -> {}{}",
                s.meta.id, s.meta.kind, from, to, note
            );
        }
        return Ok(());
    }

    if let Some(pid) = process::read_pid(&layout.pid_file()) {
        if process::is_running(pid) {
            anyhow::bail!(
                "server is running (pid {pid}); stop it with `mc-snap stop` before reverting"
            );
        }
    }

    let snap_record = match id {
        Some(id) => snapshot::load(&layout, &id)?,
        None => snapshot::latest(&layout)?
            .ok_or_else(|| anyhow::anyhow!("no snapshots to revert to"))?,
    };

    println!("reverting from snapshot {}", snap_record.meta.id);
    snapshot::restore(&layout, &snap_record)?;

    let snap = Snap::from_path(&layout.yml())?;
    let lock = mc_snap_core::lock::Lock::from_path(&layout.lock())?;

    if layout.server_dir().is_dir() {
        orchestrate::clean_stale_artifacts(&layout, &lock)?;
    }
    orchestrate::materialize(&layout, &snap, &lock).await?;

    let from = snap_record.meta.from_minecraft.as_deref().unwrap_or("?");
    println!("reverted to minecraft {from} ({} mods)", snap.mods.len());
    Ok(())
}
