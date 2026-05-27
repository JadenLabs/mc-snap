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

    if let Ok(home) = std::env::var("JAVA_HOME") {
        try_path(PathBuf::from(home).join("bin").join("java"));
    }
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            try_path(dir.join("java"));
        }
    }
    for base in linux_jvm_dirs() {
        if let Ok(rd) = std::fs::read_dir(&base) {
            for entry in rd.flatten() {
                try_path(entry.path().join("bin").join("java"));
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
    discover_all().into_iter().find(|i| i.major == required_major)
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
    let url = format!(
        "https://api.adoptium.net/v3/binary/latest/{major}/ga/{os}/{arch}/{image_type}/hotspot/normal/eclipse"
    );
    let target = jdks_root.join(major.to_string()).join(format!("{os}-{arch}"));
    std::fs::create_dir_all(&target)?;

    let client = reqwest::Client::builder()
        .user_agent(concat!("mc-snap/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let bytes = client
        .get(&url)
        .send()
        .await?
        .error_for_status()
        .context("adoptium download failed")?
        .bytes()
        .await?;

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
    let ext = if cfg!(target_os = "windows") { "zip" } else { "tar.gz" };
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
    let arch = if cfg!(target_arch = "x86_64") { "x64" } else { "aarch64" };
    format!("{os}-{arch}")
}

fn java_binary_name() -> &'static str {
    if cfg!(target_os = "windows") { "java.exe" } else { "java" }
}

fn extract_archive(archive: &Path, dest: &Path, ext: &str) -> anyhow::Result<()> {
    match ext {
        "zip" => {
            let f = std::fs::File::open(archive)?;
            let mut zf = zip::ZipArchive::new(f)?;
            zf.extract(dest)?;
            Ok(())
        }
        "tar.gz" => {
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
    let from = if nested.exists() { nested } else { inner.clone() };
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
