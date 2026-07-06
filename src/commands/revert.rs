use crate::orchestrate;
use crate::paths::ProjectLayout;
use crate::proclock::ProjectLock;
use crate::runtime::process;
use crate::snapshot;
use crate::yml::Snap;
use anyhow::Result;

pub async fn run(id: Option<String>, list: bool) -> Result<()> {
    let layout = ProjectLayout::discover(&std::env::current_dir()?)?;
    std::fs::create_dir_all(layout.snap_dir())?;

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

    let _guard = ProjectLock::acquire(&layout.lock_file())?;

    if let Some(pid) = process::read_pid(&layout.pid_file()) {
        if process::is_running_recorded(&layout.pid_file(), pid) {
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
    let lock = crate::lock::Lock::from_path(&layout.lock())?;

    if layout.server_dir_for(&snap).is_dir() {
        orchestrate::clean_stale_artifacts(&layout, &snap, &lock)?;
    }
    orchestrate::materialize(&layout, &snap, &lock, crate::cache::LinkMode::default()).await?;

    let from = snap_record.meta.from_minecraft.as_deref().unwrap_or("?");
    println!(
        "{} reverted to minecraft {from} ({} mods)",
        crate::style::ok(),
        snap.mods.len()
    );
    Ok(())
}
