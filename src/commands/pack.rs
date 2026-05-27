use anyhow::Result;
use crate::paths::ProjectLayout;
use std::io::Write;
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;

pub async fn run(out: &str) -> Result<()> {
    let layout = ProjectLayout::discover(&std::env::current_dir()?)?;
    let out_path = PathBuf::from(out);
    let f = std::fs::File::create(&out_path)?;
    let mut zw = zip::ZipWriter::new(f);
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    add_file(&mut zw, &layout.root, &layout.yml(), opts)?;
    if layout.lock().exists() {
        add_file(&mut zw, &layout.root, &layout.lock(), opts)?;
    }
    if layout.configs_dir().is_dir() {
        add_dir(&mut zw, &layout.root, &layout.configs_dir(), opts)?;
    }
    zw.finish()?;
    println!("wrote {}", out_path.display());
    Ok(())
}

fn add_file(
    zw: &mut zip::ZipWriter<std::fs::File>,
    root: &Path,
    file: &Path,
    opts: SimpleFileOptions,
) -> Result<()> {
    let name = file
        .strip_prefix(root)?
        .to_string_lossy()
        .replace('\\', "/");
    zw.start_file(name, opts)?;
    let bytes = std::fs::read(file)?;
    zw.write_all(&bytes)?;
    Ok(())
}

fn add_dir(
    zw: &mut zip::ZipWriter<std::fs::File>,
    root: &Path,
    dir: &Path,
    opts: SimpleFileOptions,
) -> Result<()> {
    for entry in walk(dir)? {
        if entry.is_file() {
            add_file(zw, root, &entry, opts)?;
        }
    }
    Ok(())
}

fn walk(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d)? {
            let entry = entry?;
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    Ok(out)
}
