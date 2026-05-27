use anyhow::{Context, Result};
use mc_snap_core::cache::ContentCache;
use mc_snap_core::download::{fetch_into_cache, http_client};
use mc_snap_core::lock::{Lock, LockLoader, LockMod};
use mc_snap_core::paths::{GlobalDirs, ProjectLayout};
use mc_snap_core::state::InstallState;
use mc_snap_core::yml::{ModEntry, Snap};
use mc_snap_core::{LoaderSpec, ResolveEnv};
use std::path::Path;
use tracing::info;

pub async fn resolve(snap: &Snap) -> Result<Lock> {
    let loader_impl = mc_snap_loaders::for_kind(&snap.server.loader.kind)?;
    info!("resolving loader {} for minecraft {}", snap.server.loader.kind, snap.server.minecraft);
    let resolved_loader = loader_impl
        .resolve(&snap.server.minecraft, &LoaderSpec(snap.server.loader.clone()))
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
        let resolved = mc_snap_providers::resolve_entry(entry, &env).await?;
        lock_mods.push(LockMod {
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
                .map(|a| mc_snap_core::lock::LockArtifact {
                    name: a.name,
                    url: a.url,
                    sha256: a.sha256,
                })
                .collect(),
        },
        mods: lock_mods,
        jdk: None,
    })
}

pub async fn materialize(layout: &ProjectLayout, snap: &Snap, lock: &Lock) -> Result<()> {
    if !snap.eula {
        anyhow::bail!(
            "Minecraft EULA not accepted. Set `eula: true` in mc-snap.yml after reading https://www.minecraft.net/en-us/eula"
        );
    }

    let globals = GlobalDirs::resolve()?;
    globals.ensure()?;
    let cache = ContentCache::new(globals.cache.clone());
    let client = http_client()?;

    let server_dir = layout.server_dir();
    std::fs::create_dir_all(&server_dir)?;
    std::fs::create_dir_all(server_dir.join("mods"))?;
    std::fs::create_dir_all(server_dir.join("config"))?;
    std::fs::create_dir_all(layout.snap_dir())?;

    let server_jar_path = cache
        .path_for(&lock.loader.server_jar_sha256)
        .to_path_buf();
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
    cache.link_into(&lock.loader.server_jar_sha256, &server_dir.join(launch_jar_name))?;

    for m in &lock.mods {
        info!("downloading mod {} {}", m.id, m.version);
        fetch_into_cache(&client, &cache, &m.url, &m.sha256).await?;
        cache.link_into(&m.sha256, &server_dir.join("mods").join(&m.filename))?;
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

    let body = crate::props::render(crate::props::default_properties(), &overrides);
    std::fs::write(server_dir.join("server.properties"), body)?;
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
    std::fs::write(path, &secret)?;
    Ok(secret)
}

fn random_secret() -> String {
    let seed = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    mc_snap_core::cache::sha256_hex(seed.as_bytes())[..32].to_string()
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
    mc_snap_core::cache::sha256_hex(s.as_bytes())
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

pub fn rcon_address(snap: &Snap) -> String {
    let port = snap
        .config
        .server_properties
        .get("rcon.port")
        .and_then(|v| v.as_i64())
        .unwrap_or(25575);
    format!("127.0.0.1:{port}")
}
