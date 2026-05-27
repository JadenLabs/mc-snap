use async_trait::async_trait;
use crate::{LaunchCtx, LoaderSpec, ResolvedLoader, ServerLoader};
use serde::Deserialize;
use std::path::Path;
use tokio::process::Command;

const META: &str = "https://meta.fabricmc.net/v2";

pub struct Fabric {
    client: reqwest::Client,
    base: String,
}

impl Default for Fabric {
    fn default() -> Self {
        Self::new()
    }
}

impl Fabric {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent(concat!("mc-snap/", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("client"),
            base: META.to_string(),
        }
    }

    pub fn with_base(base: impl Into<String>) -> Self {
        Self {
            base: base.into(),
            ..Self::new()
        }
    }
}

#[derive(Debug, Deserialize)]
struct LoaderVersion {
    version: String,
    stable: bool,
}

#[derive(Debug, Deserialize)]
struct InstallerVersion {
    version: String,
    stable: bool,
}

impl Fabric {
    async fn latest_loader(&self) -> anyhow::Result<String> {
        let v: Vec<LoaderVersion> = self
            .client
            .get(format!("{}/versions/loader", self.base))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        v.into_iter()
            .find(|x| x.stable)
            .map(|x| x.version)
            .ok_or_else(|| anyhow::anyhow!("no stable fabric loader"))
    }

    async fn latest_installer(&self) -> anyhow::Result<String> {
        let v: Vec<InstallerVersion> = self
            .client
            .get(format!("{}/versions/installer", self.base))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        v.into_iter()
            .find(|x| x.stable)
            .map(|x| x.version)
            .ok_or_else(|| anyhow::anyhow!("no stable fabric installer"))
    }
}

#[async_trait]
impl ServerLoader for Fabric {
    fn id(&self) -> &'static str {
        "fabric"
    }

    async fn resolve(&self, minecraft: &str, spec: &LoaderSpec) -> anyhow::Result<ResolvedLoader> {
        let loader = match &spec.0.version {
            Some(v) => v.clone(),
            None => self.latest_loader().await?,
        };
        let installer = match &spec.0.installer {
            Some(v) => v.clone(),
            None => self.latest_installer().await?,
        };

        let url = format!(
            "{}/versions/loader/{minecraft}/{loader}/{installer}/server/jar",
            self.base
        );
        let bytes = self
            .client
            .get(&url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        let sha256 = crate::cache::sha256_hex(&bytes);

        Ok(ResolvedLoader {
            kind: "fabric".into(),
            minecraft: minecraft.into(),
            loader_version: Some(loader),
            installer_version: Some(installer),
            server_jar_url: url,
            server_jar_sha256: sha256,
            launch_jar: "fabric-server-launch.jar".into(),
            extra: vec![],
        })
    }

    async fn install(&self, _resolved: &ResolvedLoader, server_dir: &Path) -> anyhow::Result<()> {
        std::fs::create_dir_all(server_dir.join("mods"))?;
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
