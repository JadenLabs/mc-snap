use crate::compat::{self, CompatStatus};
use anyhow::Result;
use crate::paths::ProjectLayout;
use crate::yml::Snap;

pub async fn run(target_mc: &str) -> Result<()> {
    let layout = ProjectLayout::discover(&std::env::current_dir()?)?;
    let snap = Snap::from_path(&layout.yml())?;
    let reports = compat::check_against(&snap, target_mc).await?;

    let mut ok = 0usize;
    let mut miss = 0usize;
    let mut other = 0usize;

    println!("mod compatibility against minecraft {target_mc}:");
    for r in &reports {
        match &r.status {
            CompatStatus::Compatible { suggested_version } => {
                ok += 1;
                println!("  ok    {:<32} {} -> {}", r.mod_id, r.current_version, suggested_version);
            }
            CompatStatus::Incompatible => {
                miss += 1;
                println!("  miss  {:<32} {} (no version)", r.mod_id, r.current_version);
            }
            CompatStatus::Unknown => {
                other += 1;
                println!("  url   {:<32} (url-pinned)", r.mod_id);
            }
            CompatStatus::Error(e) => {
                other += 1;
                println!("  err   {:<32} {e}", r.mod_id);
            }
        }
    }
    println!("\n{ok} compatible, {miss} missing, {other} other");
    if miss > 0 {
        std::process::exit(1);
    }
    Ok(())
}
