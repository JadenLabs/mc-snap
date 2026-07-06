use crate::paths::{GlobalDirs, ProjectLayout};
use crate::runtime::java;
use crate::style;
use crate::yml::Snap;
use anyhow::Result;

pub async fn run() -> Result<()> {
    let installs = java::discover_all();
    if installs.is_empty() {
        println!(
            "{} java: none found on system (mc-snap will download Temurin on demand)",
            style::warn_glyph()
        );
    } else {
        println!("java installs found:");
        for i in &installs {
            println!("  {} (java {})", i.bin.display(), i.major);
        }
    }

    let globals = GlobalDirs::resolve()?;
    println!("cache: {}", globals.cache.display());
    println!("jdks:  {}", globals.jdks.display());

    // If we're inside a project, report what it needs and whether we can meet it.
    let Ok(layout) = ProjectLayout::discover(&std::env::current_dir()?) else {
        return Ok(());
    };
    println!();
    match Snap::from_path(&layout.yml()) {
        Ok(snap) => {
            println!(
                "project: {} (minecraft {}, loader {})",
                style::bold(&snap.server.name),
                snap.server.minecraft,
                snap.server.loader.kind
            );
            let required = snap.runtime.java.unwrap_or(26);
            match java::find_matching(required) {
                Some(i) => println!(
                    "  {} java {} required; using {} (java {})",
                    style::ok(),
                    required,
                    i.bin.display(),
                    i.major
                ),
                None => match java::cached_temurin(&globals.jdks, required) {
                    Some(i) => println!(
                        "  {} java {} required; using cached Temurin at {}",
                        style::ok(),
                        required,
                        i.bin.display()
                    ),
                    None => println!(
                        "  {} java {} required; no matching install (Temurin will be downloaded on start)",
                        style::warn_glyph(),
                        required
                    ),
                },
            }
            if layout.lock().is_file() {
                println!("  {} lockfile present", style::ok());
            } else {
                println!(
                    "  {} no mc-snap.lock; run `mc-snap install`",
                    style::warn_glyph()
                );
            }
            if !snap.eula {
                println!(
                    "  {} eula not accepted; set `eula: true` in mc-snap.yml",
                    style::warn_glyph()
                );
            }
        }
        Err(e) => println!("{} mc-snap.yml has errors: {e:#}", style::warn_glyph()),
    }
    Ok(())
}
