use crate::orchestrate;
use crate::paths::ProjectLayout;
use crate::proclock::ProjectLock;
use crate::yml::Snap;
use anyhow::Result;

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
