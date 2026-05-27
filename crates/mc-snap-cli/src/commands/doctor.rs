use anyhow::Result;
use mc_snap_core::paths::GlobalDirs;
use mc_snap_runtime::java;

pub async fn run() -> Result<()> {
    let installs = java::discover_all();
    if installs.is_empty() {
        println!("java: none found on system");
    } else {
        println!("java installs found:");
        for i in installs {
            println!("  {} (java {})", i.bin.display(), i.major);
        }
    }
    let globals = GlobalDirs::resolve()?;
    println!("cache: {}", globals.cache.display());
    println!("jdks:  {}", globals.jdks.display());
    Ok(())
}
