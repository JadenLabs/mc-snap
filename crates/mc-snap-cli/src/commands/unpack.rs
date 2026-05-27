use anyhow::Result;
use std::path::PathBuf;

pub async fn run(bundle: &str) -> Result<()> {
    let path = PathBuf::from(bundle);
    let f = std::fs::File::open(&path)?;
    let mut zf = zip::ZipArchive::new(f)?;
    let dst = std::env::current_dir()?;
    zf.extract(&dst)?;
    println!("extracted {} to {}", path.display(), dst.display());
    Ok(())
}
