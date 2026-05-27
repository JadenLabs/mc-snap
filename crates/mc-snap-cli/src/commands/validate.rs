use anyhow::Result;
use mc_snap_core::paths::ProjectLayout;
use mc_snap_core::yml::Snap;

pub async fn run() -> Result<()> {
    let layout = ProjectLayout::discover(&std::env::current_dir()?)?;
    let snap = Snap::from_path(&layout.yml())?;
    println!("ok: {} (mc {}, loader {})", snap.server.name, snap.server.minecraft, snap.server.loader.kind);
    Ok(())
}
