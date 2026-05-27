use crate::orchestrate;
use anyhow::Result;
use mc_snap_core::paths::ProjectLayout;
use mc_snap_core::proclock::ProjectLock;
use mc_snap_core::yml::Snap;

pub async fn run() -> Result<()> {
    let layout = ProjectLayout::discover(&std::env::current_dir()?)?;
    std::fs::create_dir_all(layout.snap_dir())?;
    let _guard = ProjectLock::acquire(&layout.lock_file())?;
    let snap = Snap::from_path(&layout.yml())?;
    let lock = orchestrate::resolve(&snap).await?;
    lock.write(&layout.lock())?;
    orchestrate::materialize(&layout, &snap, &lock).await?;
    println!("installed {} ({} mods)", snap.server.name, snap.mods.len());
    Ok(())
}
