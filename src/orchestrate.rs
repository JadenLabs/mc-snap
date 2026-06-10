use crate::cache::{ContentCache, LinkMode};
use crate::download::{fetch_into_cache, http_client};
use crate::lock::{Lock, LockLoader, LockMod};
use crate::paths::{GlobalDirs, ProjectLayout};
use crate::state::InstallState;
use crate::yml::{ModEntry, Snap};
use crate::{LoaderSpec, ResolveEnv};
use anyhow::{Context, Result};
use std::path::Path;
use tracing::info;

pub async fn resolve(snap: &Snap) -> Result<Lock> {
    let loader_impl = crate::loaders::for_kind(&snap.server.loader.kind)?;
    info!(
        "resolving loader {} for minecraft {}",
        snap.server.loader.kind, snap.server.minecraft
    );
    let resolved_loader = loader_impl
        .resolve(
            &snap.server.minecraft,
            &LoaderSpec(snap.server.loader.clone()),
        )
        .await?;

    let env = ResolveEnv {
        minecraft: snap.server.minecraft.clone(),
        loader_kind: snap.server.loader.kind.clone(),
        loader_version: resolved_loader.loader_version.clone(),
    };

    let mut lock_mods = Vec::with_capacity(snap.mods.len());
    for entry in &snap.mods {
        let label = mod_label(entry);
        info!("resolving mod {label}");
        let resolved = crate::providers::resolve_entry(entry, &env).await?;
        lock_mods.push(LockMod {
            id: resolved.id,
            provider: resolved.provider,
            version: resolved.version,
            filename: resolved.filename,
            url: resolved.url,
            sha256: resolved.sha256,
        });
    }

    let mut lock_datapacks = Vec::with_capacity(snap.datapacks.len());
    for entry in &snap.datapacks {
        info!("resolving datapack {}", entry.label());
        let resolved = crate::providers::resolve_datapack(entry, &env).await?;
        lock_datapacks.push(LockMod {
            id: resolved.id,
            provider: resolved.provider,
            version: resolved.version,
            filename: resolved.filename,
            url: resolved.url,
            sha256: resolved.sha256,
        });
    }

    let yml_hash = sha256_of_string(&serde_yml::to_string(snap)?);
    Ok(Lock {
        schema: 1,
        yml_hash,
        loader: LockLoader {
            kind: resolved_loader.kind,
            minecraft: resolved_loader.minecraft,
            loader_version: resolved_loader.loader_version,
            installer_version: resolved_loader.installer_version,
            server_jar_url: resolved_loader.server_jar_url,
            server_jar_sha256: resolved_loader.server_jar_sha256,
            extra: resolved_loader
                .extra
                .into_iter()
                .map(|a| crate::lock::LockArtifact {
                    name: a.name,
                    url: a.url,
                    sha256: a.sha256,
                })
                .collect(),
        },
        mods: lock_mods,
        datapacks: lock_datapacks,
        jdk: None,
    })
}

pub async fn materialize(
    layout: &ProjectLayout,
    snap: &Snap,
    lock: &Lock,
    link_mode: LinkMode,
) -> Result<()> {
    if !snap.eula {
        anyhow::bail!(
            "Minecraft EULA not accepted. Set `eula: true` in mc-snap.yml after reading https://www.minecraft.net/en-us/eula"
        );
    }

    let globals = GlobalDirs::resolve()?;
    globals.ensure()?;
    let cache = ContentCache::new(globals.cache.clone());
    let client = http_client()?;

    let server_dir = layout.server_dir_for(snap);
    std::fs::create_dir_all(&server_dir)?;
    std::fs::create_dir_all(server_dir.join("mods"))?;
    std::fs::create_dir_all(server_dir.join("config"))?;
    std::fs::create_dir_all(layout.snap_dir())?;

    let server_jar_path = cache.path_for(&lock.loader.server_jar_sha256).to_path_buf();
    if !server_jar_path.is_file() {
        info!("downloading server jar");
        fetch_into_cache(
            &client,
            &cache,
            &lock.loader.server_jar_url,
            &lock.loader.server_jar_sha256,
        )
        .await?;
    }

    let launch_jar_name = match lock.loader.kind.as_str() {
        "fabric" => "fabric-server-launch.jar",
        _ => "server.jar",
    };
    cache.link_into(
        &lock.loader.server_jar_sha256,
        &server_dir.join(launch_jar_name),
        link_mode,
    )?;

    for m in &lock.mods {
        info!("downloading mod {} {}", m.id, m.version);
        fetch_into_cache(&client, &cache, &m.url, &m.sha256).await?;
        cache.link_into(
            &m.sha256,
            &server_dir.join("mods").join(&m.filename),
            link_mode,
        )?;
    }

    if !lock.datapacks.is_empty() {
        let datapacks_dir = server_dir.join(level_name(snap)).join("datapacks");
        std::fs::create_dir_all(&datapacks_dir)?;
        for d in &lock.datapacks {
            info!("downloading datapack {} {}", d.id, d.version);
            fetch_into_cache(&client, &cache, &d.url, &d.sha256).await?;
            cache.link_into(&d.sha256, &datapacks_dir.join(&d.filename), link_mode)?;
        }
    }

    write_eula(&server_dir)?;
    write_properties(&server_dir, snap, layout)?;
    copy_config_files(layout, snap, &server_dir)?;

    let mut state = InstallState::load(&layout.state_file()).unwrap_or_default();
    state.applied_lock_hash = Some(lock.content_hash()?);
    state.installed_at = Some(now_rfc3339());
    state.save(&layout.state_file())?;

    Ok(())
}

fn write_eula(server_dir: &Path) -> Result<()> {
    let p = server_dir.join("eula.txt");
    std::fs::write(p, "eula=true\n")?;
    Ok(())
}

fn write_properties(server_dir: &Path, snap: &Snap, layout: &ProjectLayout) -> Result<()> {
    let secret_path = layout.rcon_secret();
    let password = ensure_rcon_secret(&secret_path)?;

    let mut overrides = snap.config.server_properties.clone();
    overrides.insert("enable-rcon".into(), serde_yml::Value::Bool(true));
    overrides.insert("rcon.password".into(), serde_yml::Value::String(password));
    // Force RCON to bind to loopback regardless of what the user puts in their
    // yml - Minecraft RCON is plaintext, so the password leaks if it listens
    // externally. If a user genuinely needs remote admin they should tunnel.
    overrides.insert(
        "rcon.ip".into(),
        serde_yml::Value::String("127.0.0.1".into()),
    );

    let body = crate::props::render(crate::props::default_properties(), &overrides)?;
    let out_path = server_dir.join("server.properties");
    write_sensitive(&out_path, body.as_bytes())?;
    Ok(())
}

fn ensure_rcon_secret(path: &Path) -> Result<String> {
    if let Ok(s) = std::fs::read_to_string(path) {
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let secret = random_secret();
    write_sensitive(path, secret.as_bytes())?;
    Ok(secret)
}

/// 32 bytes of OS entropy, hex-encoded → 64 hex chars / 256 bits.
fn random_secret() -> String {
    use rand::RngCore;
    let mut buf = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    hex::encode(buf)
}

/// Write `bytes` to `path`, then on Unix lock the file down to mode 0600 so other
/// users on the box can't read the RCON password.
fn write_sensitive(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("chmod 0600 {}", path.display()))?;
    }
    Ok(())
}

fn copy_config_files(layout: &ProjectLayout, snap: &Snap, server_dir: &Path) -> Result<()> {
    for f in &snap.config.files {
        let src = layout.root.join(&f.src);
        let dst = server_dir.join(&f.dst);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&src, &dst)
            .with_context(|| format!("copying {} to {}", src.display(), dst.display()))?;
    }
    Ok(())
}

fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("@{secs}")
}

fn sha256_of_string(s: &str) -> String {
    crate::cache::sha256_hex(s.as_bytes())
}

/// Resolve the world directory name datapacks install under, honoring a
/// `level-name` override in server.properties. Defaults to Minecraft's "world".
fn level_name(snap: &Snap) -> String {
    snap.config
        .server_properties
        .get("level-name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "world".to_string())
}

fn mod_label(entry: &ModEntry) -> String {
    match entry {
        ModEntry::Registry { id, version, .. } => format!("{id} {version}"),
        ModEntry::Url { url, .. } => url.clone(),
    }
}

pub fn read_rcon_password(layout: &ProjectLayout) -> Result<String> {
    let s = std::fs::read_to_string(layout.rcon_secret())
        .context("rcon secret not found; run `mc-snap install` first")?;
    Ok(s.trim().to_string())
}

/// Remove mod jars and stale launch jars from the server dir that are not in `lock`.
/// World data and config files are left alone.
pub fn clean_stale_artifacts(layout: &ProjectLayout, snap: &Snap, lock: &Lock) -> Result<()> {
    let server_dir = layout.server_dir_for(snap);
    let mods_dir = server_dir.join("mods");
    if mods_dir.is_dir() {
        let expected: std::collections::HashSet<&str> =
            lock.mods.iter().map(|m| m.filename.as_str()).collect();
        for entry in std::fs::read_dir(&mods_dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name_s = name.to_string_lossy().to_string();
            if !expected.contains(name_s.as_str()) {
                let p = entry.path();
                if p.is_file() || p.is_symlink() {
                    std::fs::remove_file(&p)
                        .with_context(|| format!("removing stale mod {}", p.display()))?;
                }
            }
        }
    }

    let expected_launch = match lock.loader.kind.as_str() {
        "fabric" => "fabric-server-launch.jar",
        _ => "server.jar",
    };
    for name in ["fabric-server-launch.jar", "server.jar"] {
        if name == expected_launch {
            continue;
        }
        let p = server_dir.join(name);
        if p.exists() {
            std::fs::remove_file(&p)
                .with_context(|| format!("removing stale launch jar {}", p.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::{LockLoader, LockMod};
    use crate::yml::{Loader, Server};
    use tempfile::TempDir;

    fn snap_with(loader_kind: &str, location: Option<&str>) -> Snap {
        Snap {
            schema: 1,
            eula: true,
            server: Server {
                name: "test".into(),
                description: None,
                minecraft: "26.1.2".into(),
                loader: Loader {
                    kind: loader_kind.into(),
                    version: None,
                    installer: None,
                },
                location: location.map(|s| s.to_string()),
            },
            runtime: Default::default(),
            mods: vec![],
            datapacks: vec![],
            config: Default::default(),
        }
    }

    fn lock_with(loader_kind: &str, mod_filenames: &[&str]) -> Lock {
        Lock {
            schema: 1,
            yml_hash: "a".repeat(64),
            loader: LockLoader {
                kind: loader_kind.into(),
                minecraft: "26.1.2".into(),
                loader_version: None,
                installer_version: None,
                server_jar_url: "u".into(),
                server_jar_sha256: "b".repeat(64),
                extra: vec![],
            },
            mods: mod_filenames
                .iter()
                .map(|f| LockMod {
                    id: f.trim_end_matches(".jar").into(),
                    provider: "modrinth".into(),
                    version: "1.0".into(),
                    filename: (*f).into(),
                    url: "u".into(),
                    sha256: "c".repeat(64),
                })
                .collect(),
            datapacks: vec![],
            jdk: None,
        }
    }

    #[test]
    fn removes_stale_mods_and_wrong_launch_jar() {
        let td = TempDir::new().unwrap();
        let layout = ProjectLayout::at(td.path().to_path_buf());
        let snap = snap_with("fabric", None);
        let sdir = layout.server_dir_for(&snap);
        let mods = sdir.join("mods");
        std::fs::create_dir_all(&mods).unwrap();

        std::fs::write(mods.join("fabric-api.jar"), b"keep").unwrap();
        std::fs::write(mods.join("oldmod.jar"), b"stale").unwrap();
        std::fs::write(sdir.join("server.jar"), b"stale-vanilla").unwrap();
        std::fs::write(sdir.join("fabric-server-launch.jar"), b"keep-fabric").unwrap();
        std::fs::create_dir_all(sdir.join("world")).unwrap();
        std::fs::write(sdir.join("world").join("level.dat"), b"world").unwrap();

        let lock = lock_with("fabric", &["fabric-api.jar"]);
        clean_stale_artifacts(&layout, &snap, &lock).unwrap();

        assert!(mods.join("fabric-api.jar").exists(), "expected mod kept");
        assert!(!mods.join("oldmod.jar").exists(), "stale mod removed");
        assert!(
            !sdir.join("server.jar").exists(),
            "stale vanilla launch jar removed for fabric loader"
        );
        assert!(
            sdir.join("fabric-server-launch.jar").exists(),
            "expected fabric launch jar kept"
        );
        assert!(
            sdir.join("world").join("level.dat").exists(),
            "world preserved"
        );
    }

    #[test]
    fn keeps_vanilla_jar_when_loader_is_vanilla() {
        let td = TempDir::new().unwrap();
        let layout = ProjectLayout::at(td.path().to_path_buf());
        let snap = snap_with("vanilla", None);
        let sdir = layout.server_dir_for(&snap);
        std::fs::create_dir_all(sdir.join("mods")).unwrap();
        std::fs::write(sdir.join("server.jar"), b"keep").unwrap();
        std::fs::write(sdir.join("fabric-server-launch.jar"), b"stale-fabric").unwrap();

        let lock = lock_with("vanilla", &[]);
        clean_stale_artifacts(&layout, &snap, &lock).unwrap();

        assert!(sdir.join("server.jar").exists());
        assert!(!sdir.join("fabric-server-launch.jar").exists());
    }

    #[test]
    fn is_noop_when_server_dir_missing() {
        let td = TempDir::new().unwrap();
        let layout = ProjectLayout::at(td.path().to_path_buf());
        let snap = snap_with("fabric", Some("server"));
        let lock = lock_with("fabric", &["x.jar"]);
        clean_stale_artifacts(&layout, &snap, &lock).unwrap();
    }

    #[test]
    fn cleans_artifacts_in_subdir_when_location_set() {
        let td = TempDir::new().unwrap();
        let layout = ProjectLayout::at(td.path().to_path_buf());
        let snap = snap_with("fabric", Some("server"));
        let sdir = layout.server_dir_for(&snap);
        assert_eq!(sdir, td.path().join("server"));
        let mods = sdir.join("mods");
        std::fs::create_dir_all(&mods).unwrap();
        std::fs::write(mods.join("fabric-api.jar"), b"keep").unwrap();
        std::fs::write(mods.join("oldmod.jar"), b"stale").unwrap();

        let lock = lock_with("fabric", &["fabric-api.jar"]);
        clean_stale_artifacts(&layout, &snap, &lock).unwrap();

        assert!(mods.join("fabric-api.jar").exists());
        assert!(!mods.join("oldmod.jar").exists());
        assert!(
            !td.path().join(".mc-snap").join("server").exists(),
            "no legacy .mc-snap/server dir should be created"
        );
    }
}

pub fn rcon_address(snap: &Snap) -> String {
    let port = snap
        .config
        .server_properties
        .get("rcon.port")
        .and_then(|v| v.as_i64())
        .unwrap_or(25575);
    format!("127.0.0.1:{port}")
}
