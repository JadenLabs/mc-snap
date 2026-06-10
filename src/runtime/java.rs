use anyhow::Context;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct JavaInstall {
    pub bin: PathBuf,
    pub major: u32,
}

pub fn discover_all() -> Vec<JavaInstall> {
    let mut found = Vec::new();
    let mut try_path = |p: PathBuf| {
        if p.is_file() {
            if let Some(major) = probe_major(&p) {
                found.push(JavaInstall { bin: p, major });
            }
        }
    };

    let java_bin = java_binary_name();
    if let Ok(home) = std::env::var("JAVA_HOME") {
        try_path(PathBuf::from(home).join("bin").join(java_bin));
    }
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            try_path(dir.join(java_bin));
        }
    }
    for base in linux_jvm_dirs() {
        if let Ok(rd) = std::fs::read_dir(&base) {
            for entry in rd.flatten() {
                try_path(entry.path().join("bin").join(java_bin));
            }
        }
    }
    if cfg!(target_os = "macos") {
        if let Ok(rd) = std::fs::read_dir("/Library/Java/JavaVirtualMachines") {
            for entry in rd.flatten() {
                try_path(entry.path().join("Contents/Home/bin/java"));
            }
        }
    }

    dedup(found)
}

fn dedup(installs: Vec<JavaInstall>) -> Vec<JavaInstall> {
    let mut seen = std::collections::HashSet::new();
    installs
        .into_iter()
        .filter(|i| seen.insert(i.bin.clone()))
        .collect()
}

fn linux_jvm_dirs() -> Vec<&'static str> {
    vec!["/usr/lib/jvm", "/usr/lib64/jvm", "/opt"]
}

pub fn find_matching(required_major: u32) -> Option<JavaInstall> {
    // Java is backward-compatible for server use; any JDK at or above the requested
    // major will run. Prefer the lowest major that still satisfies the requirement
    // so we don't pick an unrelated newer/experimental JDK if a matching one exists.
    let mut candidates: Vec<JavaInstall> = discover_all()
        .into_iter()
        .filter(|i| i.major >= required_major)
        .collect();
    candidates.sort_by_key(|i| i.major);
    candidates.into_iter().next()
}

pub fn probe_major(bin: &Path) -> Option<u32> {
    let out = Command::new(bin).arg("-version").output().ok()?;
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    parse_major(&combined)
}

fn parse_major(s: &str) -> Option<u32> {
    let line = s.lines().find(|l| l.contains("version"))?;
    let q = line.split('"').nth(1)?;
    let first = q.split('.').next()?;
    if first == "1" {
        let second = q.split('.').nth(1)?;
        second.parse().ok()
    } else {
        first.parse().ok()
    }
}

pub fn cached_temurin(jdks_root: &Path, major: u32) -> Option<JavaInstall> {
    let target = jdks_root.join(major.to_string()).join(target_triple());
    let bin = target.join("bin").join(java_binary_name());
    if bin.is_file() {
        Some(JavaInstall { bin, major })
    } else {
        None
    }
}

pub async fn download_temurin(jdks_root: &Path, major: u32) -> anyhow::Result<JavaInstall> {
    let (os, arch, image_type, archive_ext) = adoptium_target()?;
    let target = jdks_root
        .join(major.to_string())
        .join(format!("{os}-{arch}"));
    std::fs::create_dir_all(&target)?;

    let client = reqwest::Client::builder()
        .user_agent(concat!("mc-snap/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::limited(10))
        .timeout(std::time::Duration::from_secs(300))
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()?;

    // Use the assets API to get both download URL and checksum in one request,
    // avoiding the unreliable /v3/checksum/latest/ endpoint.
    let (url, want_sha256) = fetch_adoptium_asset(&client, major, os, arch, image_type).await?;

    let bytes = client
        .get(&url)
        .send()
        .await?
        .error_for_status()
        .context("adoptium download failed")?
        .bytes()
        .await?;

    let got_sha256 = {
        use sha2::Digest;
        let mut h = sha2::Sha256::new();
        h.update(&bytes);
        hex::encode(h.finalize())
    };
    if got_sha256 != want_sha256 {
        anyhow::bail!("adoptium jdk sha256 mismatch: published {want_sha256}, got {got_sha256}");
    }

    let tmp = tempfile::NamedTempFile::new()?;
    std::fs::write(tmp.path(), &bytes)?;
    extract_archive(tmp.path(), &target, archive_ext)?;
    flatten_jdk_root(&target)?;

    let bin = target.join("bin").join(java_binary_name());
    if !bin.is_file() {
        anyhow::bail!("extracted JDK has no bin/java at {}", bin.display());
    }
    Ok(JavaInstall { bin, major })
}

async fn fetch_adoptium_asset(
    client: &reqwest::Client,
    major: u32,
    os: &str,
    arch: &str,
    image_type: &str,
) -> anyhow::Result<(String, String)> {
    let url = format!(
        "https://api.adoptium.net/v3/assets/latest/{major}/hotspot\
         ?os={os}&architecture={arch}&image_type={image_type}\
         &release_type=ga&jvm_impl=hotspot&vendor=eclipse&heap_size=normal"
    );

    let json: serde_json::Value = client
        .get(&url)
        .send()
        .await?
        .error_for_status()
        .with_context(|| format!("fetching adoptium asset listing from {url}"))?
        .json()
        .await?;

    let package = json
        .as_array()
        .and_then(|a| a.first())
        .and_then(|v| v.get("binary"))
        .and_then(|b| b.get("package"))
        .ok_or_else(|| anyhow::anyhow!("no asset found in adoptium response for java {major}"))?;

    let link = package
        .get("link")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing link in adoptium asset response"))?
        .to_string();

    let checksum = package
        .get("checksum")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing checksum in adoptium asset response"))?
        .to_lowercase();

    if checksum.len() != 64 || !checksum.chars().all(|c| c.is_ascii_hexdigit()) {
        anyhow::bail!("malformed adoptium checksum: {checksum:?}");
    }

    Ok((link, checksum))
}

fn adoptium_target() -> anyhow::Result<(&'static str, &'static str, &'static str, &'static str)> {
    let os = if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "mac"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        anyhow::bail!("unsupported os");
    };
    let arch = if cfg!(target_arch = "x86_64") {
        "x64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        anyhow::bail!("unsupported arch");
    };
    let ext = if cfg!(target_os = "windows") {
        "zip"
    } else {
        "tar.gz"
    };
    Ok((os, arch, "jdk", ext))
}

fn target_triple() -> String {
    let os = if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "mac"
    } else {
        "windows"
    };
    let arch = if cfg!(target_arch = "x86_64") {
        "x64"
    } else {
        "aarch64"
    };
    format!("{os}-{arch}")
}

fn java_binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "java.exe"
    } else {
        "java"
    }
}

fn extract_archive(archive: &Path, dest: &Path, ext: &str) -> anyhow::Result<()> {
    match ext {
        "zip" => safe_extract_zip(archive, dest),
        "tar.gz" => {
            // tar handles symlinks/permissions natively; restrict it to dest.
            let status = std::process::Command::new("tar")
                .arg("-xzf")
                .arg(archive)
                .arg("-C")
                .arg(dest)
                .status()
                .context("running tar")?;
            if !status.success() {
                anyhow::bail!("tar exit {}", status);
            }
            Ok(())
        }
        other => anyhow::bail!("unknown archive ext: {other}"),
    }
}

fn safe_extract_zip(archive: &Path, dest: &Path) -> anyhow::Result<()> {
    use std::io::Read;
    use std::path::Component;

    let f = std::fs::File::open(archive)?;
    let mut zf = zip::ZipArchive::new(f)?;
    std::fs::create_dir_all(dest)?;
    let dest_canon = dest.canonicalize().unwrap_or_else(|_| dest.to_path_buf());

    for i in 0..zf.len() {
        let mut entry = zf.by_index(i)?;
        let name = entry
            .enclosed_name()
            .ok_or_else(|| anyhow::anyhow!("zip entry {i} has an unsafe name"))?;

        if name.is_absolute() {
            anyhow::bail!("zip entry has absolute path: {}", name.display());
        }
        for c in name.components() {
            match c {
                Component::Normal(_) | Component::CurDir => {}
                _ => anyhow::bail!("zip entry has unsafe component: {}", name.display()),
            }
        }

        // Refuse symlink entries — they could point outside the destination on extraction.
        if entry
            .unix_mode()
            .map(|m| m & 0o170000 == 0o120000)
            .unwrap_or(false)
        {
            anyhow::bail!("zip contains symlink entry: {}", name.display());
        }

        let out = dest_canon.join(&name);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }

        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
            let canon = parent
                .canonicalize()
                .with_context(|| format!("canonicalize {}", parent.display()))?;
            if !canon.starts_with(&dest_canon) {
                anyhow::bail!("zip entry would escape destination: {}", out.display());
            }
        }

        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf)?;
        std::fs::write(&out, &buf)?;

        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            // Preserve executable bits for bin/java etc.
            let perm = std::fs::Permissions::from_mode(mode & 0o777);
            std::fs::set_permissions(&out, perm)?;
        }
    }
    Ok(())
}

fn flatten_jdk_root(dest: &Path) -> anyhow::Result<()> {
    if dest.join("bin").exists() {
        return Ok(());
    }
    let mut entries: Vec<_> = std::fs::read_dir(dest)?.flatten().collect();
    entries.retain(|e| e.path().is_dir());
    if entries.len() != 1 {
        return Ok(());
    }
    let inner = entries.remove(0).path();
    let nested = inner.join("Contents/Home");
    let from = if nested.exists() {
        nested
    } else {
        inner.clone()
    };
    for entry in std::fs::read_dir(&from)? {
        let entry = entry?;
        let from_path = entry.path();
        let to_path = dest.join(entry.file_name());
        std::fs::rename(&from_path, &to_path)?;
    }
    std::fs::remove_dir_all(&inner).ok();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_major;

    #[test]
    fn parses_modern_version() {
        let s = r#"openjdk version "21.0.2" 2024-01-16"#;
        assert_eq!(parse_major(s), Some(21));
    }

    #[test]
    fn parses_legacy_version() {
        let s = r#"openjdk version "1.8.0_392""#;
        assert_eq!(parse_major(s), Some(8));
    }
}
