use crate::cache::LinkMode;
use crate::lock::Lock;
use crate::orchestrate;
use crate::paths::ProjectLayout;
use crate::proclock::ProjectLock;
use crate::style;
use crate::yml::Snap;
use anyhow::Result;

pub async fn run(link_mode: LinkMode, refresh: bool) -> Result<()> {
    let layout = ProjectLayout::discover(&std::env::current_dir()?)?;
    std::fs::create_dir_all(layout.snap_dir())?;
    let _guard = ProjectLock::acquire(&layout.lock_file())?;
    let snap = Snap::from_path(&layout.yml())?;

    // Reuse the existing lockfile when it still matches the yml: that is what
    // makes `unpack` + `install` reproduce the locked versions instead of
    // re-resolving `latest` against whatever the registries serve today.
    let yml_hash = orchestrate::yml_hash(&snap)?;
    let existing = if refresh {
        None
    } else {
        Lock::from_path(&layout.lock())
            .ok()
            .filter(|l| l.yml_hash == yml_hash)
    };

    let lock = match existing {
        Some(lock) => {
            println!(
                "{} mc-snap.yml unchanged; installing from existing lockfile {}",
                style::ok(),
                style::dim("(pass --refresh to re-resolve versions)")
            );
            lock
        }
        None => {
            let lock = orchestrate::resolve(&snap).await?;
            lock.write(&layout.lock())?;
            lock
        }
    };

    orchestrate::clean_stale_artifacts(&layout, &snap, &lock)?;
    orchestrate::materialize(&layout, &snap, &lock, link_mode).await?;

    let datapacks = if lock.datapacks.is_empty() {
        String::new()
    } else {
        format!(", {} datapacks", lock.datapacks.len())
    };
    println!(
        "{} installed {} ({} mods{})",
        style::ok(),
        style::bold(&snap.server.name),
        lock.mods.len(),
        datapacks
    );
    Ok(())
}
