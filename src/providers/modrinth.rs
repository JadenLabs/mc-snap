use async_trait::async_trait;
use crate::yml::ModEntry;
use crate::{AvailableVersion, ModProvider, ModSpec, ResolveEnv, ResolvedMod};
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
    #[serde(default)]
    date_published: Option<String>,
    #[serde(default)]
    project_id: Option<String>,
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
    sha512: String,
    #[serde(default)]
    #[allow(dead_code)]
    sha1: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Project {
    slug: String,
}

impl Modrinth {
    /// Look up a Modrinth project by jar SHA-512. Returns `(slug, version_number)`
    /// on a 200, `None` on 404. Used by `init --detect` to identify jars on disk.
    pub async fn lookup_by_sha512(
        &self,
        hash: &str,
    ) -> anyhow::Result<Option<(String, String)>> {
        let url = format!("{}/version_file/{}?algorithm=sha512", self.base, hash);
        let resp = self.client.get(&url).send().await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let version: Version = resp.error_for_status()?.json().await?;
        let project_id = version
            .project_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("modrinth version response missing project_id"))?;
        let proj_url = format!("{}/project/{}", self.base, project_id);
        let project: Project = self
            .client
            .get(&proj_url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(Some((project.slug, version.version_number)))
    }
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

        let url = format!("{}/project/{}/version", self.base, id);
        let versions: Vec<Version> = self
            .client
            .get(&url)
            .query(&[
                ("loaders", format!("[\"{}\"]", env.loader_kind)),
                ("game_versions", format!("[\"{}\"]", env.minecraft)),
            ])
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

        let bytes = self
            .client
            .get(&file.url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;

        use sha2::Digest;
        let mut h = sha2::Sha512::new();
        h.update(&bytes);
        let got_sha512 = hex::encode(h.finalize());
        if got_sha512 != file.hashes.sha512 {
            anyhow::bail!(
                "modrinth sha512 mismatch for {}: api says {}, got {got_sha512}",
                file.filename,
                file.hashes.sha512
            );
        }
        let sha256 = crate::cache::sha256_hex(&bytes);

        if let Ok(globals) = crate::paths::GlobalDirs::resolve() {
            let _ = globals.ensure();
            let cache = crate::cache::ContentCache::new(globals.cache);
            if !cache.contains(&sha256) {
                let _ = cache.store(&sha256, &bytes);
            }
        }

        Ok(ResolvedMod {
            id,
            provider: "modrinth".into(),
            version: chosen.version_number.clone(),
            filename: file.filename.clone(),
            url: file.url.clone(),
            sha256,
        })
    }

    async fn list_versions(
        &self,
        spec: &ModSpec,
        env: &ResolveEnv,
    ) -> anyhow::Result<Vec<AvailableVersion>> {
        let id = match &spec.0 {
            ModEntry::Registry { id, .. } => id.clone(),
            ModEntry::Url { .. } => {
                anyhow::bail!("modrinth provider cannot list versions for url entries")
            }
        };

        let url = format!("{}/project/{}/version", self.base, id);
        let mut query: Vec<(&str, String)> =
            vec![("loaders", format!("[\"{}\"]", env.loader_kind))];
        if !env.minecraft.is_empty() {
            query.push(("game_versions", format!("[\"{}\"]", env.minecraft)));
        }
        let versions: Vec<Version> = self
            .client
            .get(&url)
            .query(&query)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(versions
            .into_iter()
            .map(|v| AvailableVersion {
                version_number: v.version_number,
                game_versions: v.game_versions,
                loaders: v.loaders,
                date_published: v.date_published,
            })
            .collect())
    }
}
