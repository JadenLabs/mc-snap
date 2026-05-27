use async_trait::async_trait;
use mc_snap_core::{LaunchCtx, LoaderSpec, ResolvedLoader, ServerLoader};
use serde::Deserialize;
use std::path::Path;
use tokio::process::Command;

const MANIFEST: &str = "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json";

pub struct Vanilla {
    client: reqwest::Client,
}

impl Default for Vanilla {
    fn default() -> Self {
        Self::new()
    }
}

impl Vanilla {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent(concat!("mc-snap/", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("client"),
        }
    }
}

#[derive(Debug, Deserialize)]
struct Manifest {
    versions: Vec<ManifestVersion>,
}

#[derive(Debug, Deserialize)]
struct ManifestVersion {
    id: String,
    url: String,
}

#[derive(Debug, Deserialize)]
struct VersionMeta {
    downloads: Downloads,
}

#[derive(Debug, Deserialize)]
struct Downloads {
    server: ServerDownload,
}

#[derive(Debug, Deserialize)]
struct ServerDownload {
    url: String,
    #[allow(dead_code)]
    sha1: String,
    #[allow(dead_code)]
    size: u64,
}

#[async_trait]
impl ServerLoader for Vanilla {
    fn id(&self) -> &'static str {
        "vanilla"
    }

    async fn resolve(&self, minecraft: &str, _spec: &LoaderSpec) -> anyhow::Result<ResolvedLoader> {
        let manifest: Manifest = self
            .client
            .get(MANIFEST)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let v = manifest
            .versions
            .into_iter()
            .find(|v| v.id == minecraft)
            .ok_or_else(|| anyhow::anyhow!("unknown minecraft version: {minecraft}"))?;
        let meta: VersionMeta = self
            .client
            .get(&v.url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let bytes = self
            .client
            .get(&meta.downloads.server.url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        let sha256 = mc_snap_core::cache::sha256_hex(&bytes);

        Ok(ResolvedLoader {
            kind: "vanilla".into(),
            minecraft: minecraft.into(),
            loader_version: None,
            installer_version: None,
            server_jar_url: meta.downloads.server.url,
            server_jar_sha256: sha256,
            launch_jar: "server.jar".into(),
            extra: vec![],
        })
    }

    async fn install(&self, _resolved: &ResolvedLoader, _server_dir: &Path) -> anyhow::Result<()> {
        Ok(())
    }

    fn launch_command(&self, ctx: &LaunchCtx) -> Command {
        let mem = ctx.memory.clone();
        let mut cmd = Command::new(&ctx.java_bin);
        cmd.current_dir(&ctx.server_dir)
            .arg(format!("-Xms{mem}"))
            .arg(format!("-Xmx{mem}"));
        for f in &ctx.extra_flags {
            cmd.arg(f);
        }
        cmd.arg("-jar").arg(&ctx.launch_jar).arg("nogui");
        cmd
    }
}
