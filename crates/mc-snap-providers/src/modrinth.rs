use async_trait::async_trait;
use mc_snap_core::yml::ModEntry;
use mc_snap_core::{ModProvider, ModSpec, ResolveEnv, ResolvedMod};
use serde::Deserialize;

const API: &str = "https://api.modrinth.com/v2";

pub struct Modrinth {
    client: reqwest::Client,
    base: String,
}

impl Default for Modrinth {
    fn default() -> Self {
        Self::new()
    }
}

impl Modrinth {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent(concat!("mc-snap/", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("client"),
            base: API.to_string(),
        }
    }

    pub fn with_base(base: impl Into<String>) -> Self {
        Self { base: base.into(), ..Self::new() }
    }
}

#[derive(Debug, Deserialize)]
struct Version {
    id: String,
    version_number: String,
    game_versions: Vec<String>,
    loaders: Vec<String>,
    files: Vec<VersionFile>,
}

#[derive(Debug, Deserialize)]
struct VersionFile {
    url: String,
    filename: String,
    hashes: Hashes,
    #[serde(default)]
    primary: bool,
}

#[derive(Debug, Deserialize)]
struct Hashes {
    sha256: String,
}

#[async_trait]
impl ModProvider for Modrinth {
    fn id(&self) -> &'static str {
        "modrinth"
    }

    async fn resolve(&self, spec: &ModSpec, env: &ResolveEnv) -> anyhow::Result<ResolvedMod> {
        let (id, version) = match &spec.0 {
            ModEntry::Registry { id, version, .. } => (id.clone(), version.clone()),
            ModEntry::Url { .. } => anyhow::bail!("modrinth provider cannot resolve url entries"),
        };

        let url = format!(
            "{}/project/{}/version?loaders=[\"{}\"]&game_versions=[\"{}\"]",
            self.base, id, env.loader_kind, env.minecraft
        );
        let versions: Vec<Version> = self
            .client
            .get(&url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let chosen = if version == "latest" {
            versions.into_iter().next()
        } else {
            versions
                .into_iter()
                .find(|v| v.version_number == version || v.id == version)
        }
        .ok_or_else(|| anyhow::anyhow!("no matching modrinth version for {id} {version}"))?;

        if !chosen.game_versions.iter().any(|g| g == &env.minecraft) {
            anyhow::bail!(
                "modrinth version {} does not support minecraft {}",
                chosen.version_number,
                env.minecraft
            );
        }
        if !chosen.loaders.iter().any(|l| l == &env.loader_kind) {
            anyhow::bail!(
                "modrinth version {} does not support loader {}",
                chosen.version_number,
                env.loader_kind
            );
        }

        let file = chosen
            .files
            .iter()
            .find(|f| f.primary)
            .or_else(|| chosen.files.first())
            .ok_or_else(|| anyhow::anyhow!("modrinth version {} has no files", chosen.id))?;

        Ok(ResolvedMod {
            id,
            provider: "modrinth".into(),
            version: chosen.version_number.clone(),
            filename: file.filename.clone(),
            url: file.url.clone(),
            sha256: file.hashes.sha256.clone(),
        })
    }
}
