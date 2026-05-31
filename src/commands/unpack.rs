use anyhow::{bail, Context, Result};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

const MAX_BUNDLE_FILE_BYTES: u64 = 256 * 1024 * 1024;

pub async fn run(bundle: &str) -> Result<()> {
    let path = PathBuf::from(bundle);
    let f =
        std::fs::File::open(&path).with_context(|| format!("opening bundle {}", path.display()))?;
    let mut zf = zip::ZipArchive::new(f)?;
    let dst = std::env::current_dir()?;
    let dst_canon = dst.canonicalize().unwrap_or(dst.clone());

    for i in 0..zf.len() {
        let mut entry = zf.by_index(i)?;
        let raw_name = entry
            .enclosed_name()
            .with_context(|| format!("entry {} has an unsafe name", i))?;
        validate_entry_path(&raw_name)?;

        let out_path = dst_canon.join(&raw_name);

        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)?;
            continue;
        }

        // Reject symlinks outright — they're a vector for escaping the destination.
        if is_symlink(&entry) {
            bail!(
                "bundle contains a symlink entry ({}), refusing to extract",
                raw_name.display()
            );
        }

        if entry.size() > MAX_BUNDLE_FILE_BYTES {
            bail!(
                "bundle entry {} exceeds size cap ({} > {} bytes)",
                raw_name.display(),
                entry.size(),
                MAX_BUNDLE_FILE_BYTES
            );
        }

        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        ensure_under(&dst_canon, &out_path)?;

        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf)?;
        std::fs::write(&out_path, &buf)?;
    }

    println!("extracted {} to {}", path.display(), dst.display());
    Ok(())
}

fn validate_entry_path(p: &Path) -> Result<()> {
    if p.is_absolute() {
        bail!("zip entry has absolute path: {}", p.display());
    }
    for c in p.components() {
        match c {
            Component::Normal(_) => {}
            Component::CurDir => {}
            _ => bail!("zip entry has unsafe component: {}", p.display()),
        }
    }
    Ok(())
}

fn ensure_under(root: &Path, child: &Path) -> Result<()> {
    // Canonicalize the parent that actually exists; the leaf may not exist yet.
    let parent = child.parent().unwrap_or(child);
    let canon = parent
        .canonicalize()
        .with_context(|| format!("canonicalize {}", parent.display()))?;
    if !canon.starts_with(root) {
        bail!("zip entry would escape destination: {}", child.display());
    }
    Ok(())
}

fn is_symlink(entry: &zip::read::ZipFile) -> bool {
    // Unix mode bits encode file type. 0o120000 = symlink.
    entry
        .unix_mode()
        .map(|m| m & 0o170000 == 0o120000)
        .unwrap_or(false)
}
