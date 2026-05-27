use async_trait::async_trait;
use mc_snap_core::{LaunchCtx, LoaderSpec, ResolvedLoader, ServerLoader};
use serde::Deserialize;
use sha1::{Digest, Sha1};
use std::path::Path;
use tokio::process::Command;

fn sha1_hex(bytes: &[u8]) -> String {
    let mut h = Sha1::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

const MANIFEST: &str = "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json";

pub struct Vanilla {
    client: reqwest::Client,
    manifest_url: String,
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
            manifest_url: MANIFEST.to_string(),
        }
    }

    pub fn with_manifest_url(url: impl Into<String>) -> Self {
        Self {
            manifest_url: url.into(),
            ..Self::new()
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
            .get(&self.manifest_url)
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
        // Verify Mojang's published sha1 before we trust the bytes. Without this the
        // lockfile's sha256 is just "the hash of whatever the server returned", which
        // proves nothing.
        let want_sha1 = meta.downloads.server.sha1.trim().to_lowercase();
        let got_sha1 = sha1_hex(&bytes);
        if got_sha1 != want_sha1 {
            anyhow::bail!(
                "vanilla server jar sha1 mismatch for {}: manifest says {}, got {}",
                minecraft, want_sha1, got_sha1
            );
        }
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
