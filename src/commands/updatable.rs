use crate::compat::{self, CompatStatus};
use anyhow::Result;
use crate::paths::ProjectLayout;
use crate::yml::Snap;

pub async fn run(target_mc: Option<String>) -> Result<()> {
    let layout = ProjectLayout::discover(&std::env::current_dir()?)?;
    let snap = Snap::from_path(&layout.yml())?;

    match target_mc {
        Some(target) => answer_for_target(&snap, &target).await,
        None => suggest_target(&snap).await,
    }
}

async fn answer_for_target(snap: &Snap, target: &str) -> Result<()> {
    if compat::compare_mc_version(target, &snap.server.minecraft) != std::cmp::Ordering::Greater {
        println!(
            "target {target} is not newer than current {}; nothing to do",
            snap.server.minecraft
        );
        return Ok(());
    }
    let reports = compat::check_against(snap, target).await?;
    let missing: Vec<&str> = reports
        .iter()
        .filter(|r| matches!(r.status, CompatStatus::Incompatible))
        .map(|r| r.mod_id.as_str())
        .collect();
    if missing.is_empty() {
        println!(
            "yes: all {} registry mods have a version for minecraft {target}",
            reports
                .iter()
                .filter(|r| matches!(r.status, CompatStatus::Compatible { .. }))
                .count()
        );
        println!("run: mc-snap update --to {target}");
    } else {
        println!(
            "no: {} mod(s) lack a {target} version: {}",
            missing.len(),
            missing.join(", ")
        );
        println!("run: mc-snap update --to {target} --skip-missing");
        std::process::exit(1);
    }
    Ok(())
}

async fn suggest_target(snap: &Snap) -> Result<()> {
    let per_mod = compat::collect_supported_mc_versions(snap).await?;
    if per_mod.is_empty() {
        println!("no registry mods; nothing to check");
        return Ok(());
    }
    match compat::newest_common_mc(&per_mod, &snap.server.minecraft) {
        Some(v) => {
            println!(
                "current: {}\nnewest minecraft supported by all {} registry mods: {v}",
                snap.server.minecraft,
                per_mod.len()
            );
            println!("run: mc-snap update --to {v}");
        }
        None => {
            println!(
                "no newer minecraft version is supported by every mod; run `mc-snap check --to <ver>` to see per-mod gaps"
            );
        }
    }
    Ok(())
}
