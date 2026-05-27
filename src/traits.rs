use crate::yml::{Loader, ModEntry};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct LoaderSpec(pub Loader);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedLoader {
    pub kind: String,
    pub minecraft: String,
    pub loader_version: Option<String>,
    pub installer_version: Option<String>,
    pub server_jar_url: String,
    pub server_jar_sha256: String,
    pub launch_jar: String,
    pub extra: Vec<ResolvedArtifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedArtifact {
    pub name: String,
    pub url: String,
    pub sha256: String,
    pub install_path: String,
}

#[derive(Debug, Clone)]
pub struct LaunchCtx {
    pub server_dir: PathBuf,
    pub launch_jar: PathBuf,
    pub java_bin: PathBuf,
    pub memory: String,
    pub extra_flags: Vec<String>,
}

#[async_trait]
pub trait ServerLoader: Send + Sync {
    fn id(&self) -> &'static str;
    async fn resolve(&self, minecraft: &str, spec: &LoaderSpec) -> anyhow::Result<ResolvedLoader>;
    async fn install(&self, resolved: &ResolvedLoader, server_dir: &Path) -> anyhow::Result<()>;
    fn launch_command(&self, ctx: &LaunchCtx) -> tokio::process::Command;
}

#[derive(Debug, Clone)]
pub struct ModSpec(pub ModEntry);

#[derive(Debug, Clone)]
pub struct ResolveEnv {
    pub minecraft: String,
    pub loader_kind: String,
    pub loader_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedMod {
    pub id: String,
    pub provider: String,
    pub version: String,
    pub filename: String,
    pub url: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailableVersion {
    pub version_number: String,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub date_published: Option<String>,
}

#[async_trait]
pub trait ModProvider: Send + Sync {
    fn id(&self) -> &'static str;
    async fn resolve(&self, spec: &ModSpec, env: &ResolveEnv) -> anyhow::Result<ResolvedMod>;

    /// List all known versions of this mod. `env.minecraft` may be empty to mean "any";
    /// providers should ignore game_versions filtering when it is empty.
    async fn list_versions(
        &self,
        _spec: &ModSpec,
        _env: &ResolveEnv,
    ) -> anyhow::Result<Vec<AvailableVersion>> {
        anyhow::bail!("provider does not support version listing")
    }
}
