use crate::compat;
use anyhow::Result;
use crate::lock::Lock;
use crate::paths::ProjectLayout;
use crate::yml::Snap;

pub async fn run() -> Result<()> {
    let layout = ProjectLayout::discover(&std::env::current_dir()?)?;
    let snap = Snap::from_path(&layout.yml())?;
    if !layout.lock().is_file() {
        anyhow::bail!("no mc-snap.lock; run `mc-snap install` first");
    }
    let lock = Lock::from_path(&layout.lock())?;

    let reports = compat::find_newer_versions(&snap, &lock).await?;
    if reports.is_empty() {
        println!("no registry mods to check");
        return Ok(());
    }

    println!("newer mod versions available for minecraft {}:", snap.server.minecraft);
    let mut any = false;
    for r in &reports {
        if r.newer.is_empty() {
            println!("  up-to-date  {:<32} {}", r.mod_id, r.current_version);
        } else {
            any = true;
            let preview = r.newer.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
            let extra = if r.newer.len() > 3 {
                format!(" (+{} more)", r.newer.len() - 3)
            } else {
                String::new()
            };
            println!(
                "  newer       {:<32} {} -> {}{}",
                r.mod_id, r.current_version, preview, extra
            );
        }
    }
    if any {
        println!("\nedit mc-snap.yml to pin a newer version, then `mc-snap install`.");
    }
    Ok(())
}
