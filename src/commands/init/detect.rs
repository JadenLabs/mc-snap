use anyhow::Result;
use serde_yml::{Mapping, Value};
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::providers::modrinth::Modrinth;
use crate::yml::ModEntry;

#[derive(Debug, Default)]
pub struct Detected {
    pub name: Option<String>,
    pub minecraft: Option<String>,
    pub loader: Option<&'static str>,
    pub fabric_loader_version: Option<String>,
    pub java_major: Option<u32>,
    pub memory: Option<String>,
    pub eula: bool,
    pub server_properties: Mapping,
    pub mods: Vec<ModEntry>,
    pub unresolved_mods: Vec<String>,
    pub unsupported_loader: Option<&'static str>,
    /// Relative path from the cwd (where `mc-snap.yml` is written) to the detected
    /// server directory. `None` when the detect path *is* the cwd.
    pub detected_location: Option<String>,
}

pub async fn detect(root: &Path, resolve_mods: bool) -> Result<Detected> {
    let mut d = Detected {
        name: root
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string()),
        detected_location: compute_detected_location(root),
        ..Default::default()
    };

    if let Some(unsup) = detect_unsupported_loader(root) {
        d.unsupported_loader = Some(unsup);
    } else {
        detect_loader_and_version(root, &mut d);
    }

    if let Some(props) = parse_server_properties(&root.join("server.properties")) {
        d.server_properties = props;
    }

    d.eula = parse_eula(&root.join("eula.txt"));

    if let Some(mem) = detect_memory(root) {
        d.memory = Some(mem);
    }

    if let Some(major) = pick_java(d.minecraft.as_deref()) {
        d.java_major = Some(major);
    }

    let mods_dir = root.join("mods");
    if mods_dir.is_dir() {
        let (entries, unresolved) = detect_mods(&mods_dir, resolve_mods).await?;
        d.mods = entries;
        d.unresolved_mods = unresolved;
    }

    Ok(d)
}

fn compute_detected_location(root: &Path) -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    let abs_root = root.canonicalize().unwrap_or_else(|_| {
        if root.is_absolute() {
            root.to_path_buf()
        } else {
            cwd.join(root)
        }
    });
    let abs_cwd = cwd.canonicalize().unwrap_or(cwd);
    if abs_root == abs_cwd {
        return None;
    }
    let rel = abs_root.strip_prefix(&abs_cwd).ok()?;
    let s = rel.to_string_lossy().to_string();
    if s.is_empty() || s == "." {
        None
    } else {
        Some(s)
    }
}

fn detect_unsupported_loader(root: &Path) -> Option<&'static str> {
    if root.join("libraries/net/minecraftforge").is_dir()
        || glob_first(root, "forge-").is_some()
    {
        return Some("forge");
    }
    if root.join("libraries/net/neoforged").is_dir() || glob_first(root, "neoforge-").is_some() {
        return Some("neoforge");
    }
    if glob_first(root, "paper-").is_some() || root.join("paper.yml").is_file() {
        return Some("paper");
    }
    if glob_first(root, "spigot-").is_some() || glob_first(root, "spigot.jar").is_some() {
        return Some("spigot");
    }
    None
}

fn glob_first(dir: &Path, prefix: &str) -> Option<PathBuf> {
    let rd = std::fs::read_dir(dir).ok()?;
    for e in rd.flatten() {
        if let Some(name) = e.file_name().to_str() {
            if name.starts_with(prefix) && name.ends_with(".jar") {
                return Some(e.path());
            }
        }
    }
    None
}

fn detect_loader_and_version(root: &Path, d: &mut Detected) {
    // Fabric detection first — the launcher.properties points at the embedded vanilla jar.
    let launcher_props = root.join("fabric-server-launcher.properties");
    let fabric_launch = root.join("fabric-server-launch.jar");
    if launcher_props.is_file() || fabric_launch.is_file() {
        d.loader = Some("fabric");
        if let Some(ver) = read_fabric_loader_version(root) {
            d.fabric_loader_version = Some(ver);
        }
        // Try to find the embedded vanilla jar referenced by the launcher properties.
        if let Some(server_jar) = fabric_server_jar(&launcher_props, root) {
            if let Some(mc) = read_mc_version_from_jar(&server_jar) {
                d.minecraft = Some(mc);
                return;
            }
        }
        if let Some(mc) = read_mc_version_from_jar(&fabric_launch) {
            d.minecraft = Some(mc);
            return;
        }
        // Fall through to other jar probes.
    }

    // Vanilla / generic server.jar
    for candidate in ["server.jar", "minecraft_server.jar"] {
        let p = root.join(candidate);
        if p.is_file() {
            if let Some(mc) = read_mc_version_from_jar(&p) {
                d.minecraft = Some(mc);
                if d.loader.is_none() {
                    d.loader = Some("vanilla");
                }
                return;
            }
        }
    }

    // Any *.jar that looks like a server bundle.
    if let Ok(rd) = std::fs::read_dir(root) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().and_then(|s| s.to_str()) == Some("jar") {
                if let Some(mc) = read_mc_version_from_jar(&p) {
                    d.minecraft = Some(mc);
                    if d.loader.is_none() {
                        d.loader = Some("vanilla");
                    }
                    return;
                }
            }
        }
    }
}

fn read_fabric_loader_version(root: &Path) -> Option<String> {
    let fabric_dir = root.join(".fabric");
    if !fabric_dir.is_dir() {
        return None;
    }
    let rd = std::fs::read_dir(&fabric_dir).ok()?;
    for e in rd.flatten() {
        if !e.file_type().ok()?.is_dir() {
            continue;
        }
        let name = e.file_name();
        let s = name.to_str()?;
        // Layout is typically `.fabric/server/<loader-version>/`.
        if s == "server" || s == "client" {
            let inner = std::fs::read_dir(e.path()).ok()?;
            for sub in inner.flatten() {
                if let Some(v) = sub.file_name().to_str() {
                    if v.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                        return Some(v.to_string());
                    }
                }
            }
        }
    }
    None
}

fn fabric_server_jar(launcher_props: &Path, root: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(launcher_props).ok()?;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == "serverJar" {
                let val = v.trim();
                let p = root.join(val);
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    None
}

fn read_mc_version_from_jar(jar: &Path) -> Option<String> {
    let file = std::fs::File::open(jar).ok()?;
    let mut zip = zip::ZipArchive::new(file).ok()?;
    let mut entry = zip.by_name("version.json").ok()?;
    let mut buf = String::new();
    entry.read_to_string(&mut buf).ok()?;
    let v: serde_json::Value = serde_json::from_str(&buf).ok()?;
    // Modern releases use `name`; fall back to `id`.
    v.get("name")
        .or_else(|| v.get("id"))
        .and_then(|s| s.as_str())
        .map(|s| s.to_string())
}

fn parse_server_properties(path: &Path) -> Option<Mapping> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut m = Mapping::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let key = k.trim();
        let value = v.trim();
        if key.is_empty() {
            continue;
        }
        m.insert(Value::String(key.to_string()), coerce_scalar(value));
    }
    if m.is_empty() {
        None
    } else {
        Some(m)
    }
}

fn coerce_scalar(s: &str) -> Value {
    if s.is_empty() {
        return Value::String(String::new());
    }
    if s.eq_ignore_ascii_case("true") {
        return Value::Bool(true);
    }
    if s.eq_ignore_ascii_case("false") {
        return Value::Bool(false);
    }
    if let Ok(n) = s.parse::<i64>() {
        return Value::Number(n.into());
    }
    Value::String(s.to_string())
}

fn parse_eula(path: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            if k.trim().eq_ignore_ascii_case("eula") && v.trim().eq_ignore_ascii_case("true") {
                return true;
            }
        }
    }
    false
}

fn detect_memory(root: &Path) -> Option<String> {
    for script in ["start.sh", "run.sh", "start.bat", "start.cmd", "run.bat"] {
        let p = root.join(script);
        if let Ok(text) = std::fs::read_to_string(&p) {
            if let Some(mem) = extract_xmx(&text) {
                return Some(mem);
            }
        }
    }
    None
}

fn extract_xmx(text: &str) -> Option<String> {
    // Match -Xmx<digits><unit?>. Unit may be G, g, M, m, K, k.
    let bytes = text.as_bytes();
    let needle = b"-Xmx";
    let mut i = 0;
    while i + needle.len() <= bytes.len() {
        if &bytes[i..i + needle.len()] == needle {
            let mut j = i + needle.len();
            let start = j;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j > start {
                let mut end = j;
                if end < bytes.len() && matches!(bytes[end], b'G' | b'g' | b'M' | b'm' | b'K' | b'k')
                {
                    end += 1;
                }
                let digits = std::str::from_utf8(&bytes[start..j]).ok()?;
                let unit = if end > j {
                    (bytes[j] as char).to_ascii_uppercase().to_string()
                } else {
                    String::new()
                };
                return Some(format!("{digits}{unit}"));
            }
        }
        i += 1;
    }
    None
}

fn pick_java(minecraft: Option<&str>) -> Option<u32> {
    let required = required_java_major(minecraft);
    let installs = crate::runtime::java::discover_all();
    let mut matching: Vec<u32> = installs
        .into_iter()
        .map(|i| i.major)
        .filter(|m| *m >= required)
        .collect();
    matching.sort_unstable();
    matching.into_iter().next().or(Some(required))
}

fn required_java_major(minecraft: Option<&str>) -> u32 {
    let Some(v) = minecraft else { return 21 };
    let mut parts = v.split('.').filter_map(|p| p.parse::<u32>().ok());
    let major = parts.next().unwrap_or(0);
    let minor = parts.next().unwrap_or(0);
    if major >= 2 {
        // Future-proof for hypothetical 2.x line; treat the same as 1.21+.
        return 21;
    }
    match minor {
        0..=16 => 8,
        17..=18 => 17,
        19 => 17,
        20 => 21,
        _ => 21,
    }
}

async fn detect_mods(
    mods_dir: &Path,
    resolve: bool,
) -> Result<(Vec<ModEntry>, Vec<String>)> {
    let mut jars: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(mods_dir)?.flatten() {
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) == Some("jar") {
            jars.push(p);
        }
    }
    jars.sort();

    if !resolve {
        let names = jars
            .iter()
            .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(|s| s.to_string()))
            .collect();
        return Ok((Vec::new(), names));
    }

    let modrinth = Modrinth::new();
    let mut resolved = Vec::new();
    let mut unresolved = Vec::new();
    for jar in &jars {
        let hash = sha512_file(jar)?;
        let filename = jar
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();
        match modrinth.lookup_by_sha512(&hash).await {
            Ok(Some((slug, version_number))) => {
                resolved.push(ModEntry::Registry {
                    id: slug,
                    provider: "modrinth".to_string(),
                    version: version_number,
                });
            }
            Ok(None) => unresolved.push(filename),
            Err(e) => {
                tracing::warn!("modrinth lookup failed for {filename}: {e:#}");
                unresolved.push(filename);
            }
        }
    }
    Ok((resolved, unresolved))
}

fn sha512_file(path: &Path) -> Result<String> {
    use sha2::Digest;
    let bytes = std::fs::read(path)?;
    let mut h = sha2::Sha512::new();
    h.update(&bytes);
    Ok(hex::encode(h.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coerce_scalar_handles_types() {
        assert_eq!(coerce_scalar("true"), Value::Bool(true));
        assert_eq!(coerce_scalar("False"), Value::Bool(false));
        assert_eq!(coerce_scalar("20"), Value::Number(20i64.into()));
        assert_eq!(coerce_scalar("hi"), Value::String("hi".into()));
        assert_eq!(coerce_scalar(""), Value::String("".into()));
    }

    #[test]
    fn parses_server_properties() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("server.properties");
        std::fs::write(
            &p,
            "# header comment\nmotd=Hello\nmax-players=20\nonline-mode=true\n",
        )
        .unwrap();
        let m = parse_server_properties(&p).unwrap();
        assert_eq!(m[Value::String("motd".into())], Value::String("Hello".into()));
        assert_eq!(
            m[Value::String("max-players".into())],
            Value::Number(20i64.into())
        );
        assert_eq!(
            m[Value::String("online-mode".into())],
            Value::Bool(true)
        );
    }

    #[test]
    fn parses_eula_true() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("eula.txt");
        std::fs::write(&p, "# EULA header\neula=true\n").unwrap();
        assert!(parse_eula(&p));
    }

    #[test]
    fn parses_eula_false() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("eula.txt");
        std::fs::write(&p, "eula=false\n").unwrap();
        assert!(!parse_eula(&p));
    }

    #[test]
    fn extracts_xmx_with_unit() {
        assert_eq!(extract_xmx("java -Xmx4G -jar server.jar"), Some("4G".into()));
        assert_eq!(extract_xmx("-Xmx2048M"), Some("2048M".into()));
        assert_eq!(extract_xmx("-Xmx1024"), Some("1024".into()));
        assert_eq!(extract_xmx("no flag here"), None);
    }

    #[test]
    fn java_major_matches_mc() {
        assert_eq!(required_java_major(Some("1.16.5")), 8);
        assert_eq!(required_java_major(Some("1.18.2")), 17);
        assert_eq!(required_java_major(Some("1.20.4")), 21);
        assert_eq!(required_java_major(Some("1.21.1")), 21);
        assert_eq!(required_java_major(None), 21);
    }

    #[test]
    fn detects_eula_and_properties_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("eula.txt"), "eula=true").unwrap();
        std::fs::write(
            dir.path().join("server.properties"),
            "motd=hi\nmax-players=12\n",
        )
        .unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let d = rt.block_on(detect(dir.path(), false)).unwrap();
        assert!(d.eula);
        assert_eq!(
            d.server_properties[Value::String("max-players".into())],
            Value::Number(12i64.into())
        );
    }

    #[test]
    fn detects_unsupported_paper_jar() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("paper-1.20.4.jar"), b"fake").unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let d = rt.block_on(detect(dir.path(), false)).unwrap();
        assert_eq!(d.unsupported_loader, Some("paper"));
    }
}
