use crate::yml::ModEntry;
use crate::{AvailableVersion, ModProvider, ModSpec, ResolveEnv, ResolvedMod};
use async_trait::async_trait;
use serde::Deserialize;

const API: &str = "https://api.curseforge.com/v1";
/// CurseForge's numeric game id for Minecraft.
const GAME_ID: u32 = 432;
/// classId values used when resolving a slug to a numeric project id.
const CLASS_MODS: u32 = 6;
const CLASS_DATAPACKS: u32 = 6945;

/// CurseForge mod/datapack provider. The CurseForge v1 API requires a personal
/// API key passed in the `x-api-key` header; we read it from `CURSEFORGE_API_KEY`
/// (or `CF_API_KEY`). When `env.loader_kind` is `"datapack"` the provider
/// resolves against the Data Packs class and skips loader filtering.
pub struct CurseForge {
    client: reqwest::Client,
    base: String,
    api_key: Option<String>,
}

impl Default for CurseForge {
    fn default() -> Self {
        Self::new()
    }
}

impl CurseForge {
    pub fn new() -> Self {
        let api_key = std::env::var("CURSEFORGE_API_KEY")
            .or_else(|_| std::env::var("CF_API_KEY"))
            .ok()
            .filter(|s| !s.trim().is_empty());
        Self {
            client: reqwest::Client::builder()
                .user_agent(concat!("mc-snap/", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("client"),
            base: API.to_string(),
            api_key,
        }
    }

    /// Construct against a stub base URL with a fixed test key. Used by integration tests.
    pub fn with_base(base: impl Into<String>) -> Self {
        Self {
            base: base.into(),
            api_key: Some("test-key".to_string()),
            ..Self::new()
        }
    }

    fn key(&self) -> anyhow::Result<&str> {
        self.api_key.as_deref().ok_or_else(|| {
            anyhow::anyhow!(
                "curseforge provider needs an API key; set CURSEFORGE_API_KEY (get one at https://console.curseforge.com)"
            )
        })
    }

    fn get(&self, url: &str) -> anyhow::Result<reqwest::RequestBuilder> {
        Ok(self.client.get(url).header("x-api-key", self.key()?))
    }

    /// Resolve a `id` field that is either a numeric project id or a slug into a
    /// numeric project id. Datapacks live in a different class than mods.
    async fn resolve_project_id(&self, id: &str, datapack: bool) -> anyhow::Result<u64> {
        if let Ok(n) = id.parse::<u64>() {
            return Ok(n);
        }
        let class_id = if datapack {
            CLASS_DATAPACKS
        } else {
            CLASS_MODS
        };
        let url = format!("{}/mods/search", self.base);
        let resp: SearchResponse = self
            .get(&url)?
            .query(&[
                ("gameId", GAME_ID.to_string()),
                ("classId", class_id.to_string()),
                ("slug", id.to_string()),
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        resp.data
            .into_iter()
            .next()
            .map(|m| m.id)
            .ok_or_else(|| anyhow::anyhow!("no curseforge project found for slug {id}"))
    }

    async fn fetch_files(&self, project_id: u64, env: &ResolveEnv) -> anyhow::Result<Vec<CfFile>> {
        let url = format!("{}/mods/{}/files", self.base, project_id);
        let mut query: Vec<(&str, String)> = vec![("pageSize", "50".to_string())];
        if !env.minecraft.is_empty() {
            query.push(("gameVersion", env.minecraft.clone()));
        }
        if let Some(t) = mod_loader_type(&env.loader_kind) {
            query.push(("modLoaderType", t.to_string()));
        }
        let resp: FilesResponse = self
            .get(&url)?
            .query(&query)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(resp.data)
    }
}

/// Map a loader kind to CurseForge's `modLoaderType` enum. `None` means "do not
/// filter by loader" (vanilla mods and datapacks aren't loader-specific).
fn mod_loader_type(loader_kind: &str) -> Option<u32> {
    match loader_kind {
        "forge" => Some(1),
        "fabric" => Some(4),
        "quilt" => Some(5),
        "neoforge" => Some(6),
        _ => None,
    }
}

/// CurseForge display names that denote a loader rather than a Minecraft version.
const LOADER_GAME_VERSIONS: &[&str] =
    &["forge", "fabric", "quilt", "neoforge", "rift", "liteloader"];

fn split_game_versions(raw: &[String]) -> (Vec<String>, Vec<String>) {
    let mut mc = Vec::new();
    let mut loaders = Vec::new();
    for v in raw {
        if LOADER_GAME_VERSIONS.contains(&v.to_ascii_lowercase().as_str()) {
            loaders.push(v.to_ascii_lowercase());
        } else {
            mc.push(v.clone());
        }
    }
    (mc, loaders)
}

/// Build the canonical forgecdn download URL from a file id + name, used when the
/// API omits `downloadUrl` (the author opted out of third-party API downloads but
/// the CDN object still exists at the deterministic path).
fn cdn_url(file_id: u64, filename: &str) -> String {
    let encoded = filename.replace(' ', "%20");
    format!(
        "https://edge.forgecdn.net/files/{}/{:03}/{}",
        file_id / 1000,
        file_id % 1000,
        encoded
    )
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    data: Vec<SearchMod>,
}

#[derive(Debug, Deserialize)]
struct SearchMod {
    id: u64,
}

#[derive(Debug, Deserialize)]
struct FilesResponse {
    data: Vec<CfFile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfFile {
    id: u64,
    display_name: String,
    file_name: String,
    #[serde(default)]
    download_url: Option<String>,
    #[serde(default)]
    hashes: Vec<CfHash>,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    file_date: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CfHash {
    value: String,
    algo: u32,
}

impl CfFile {
    fn sha1(&self) -> Option<&str> {
        self.hashes
            .iter()
            .find(|h| h.algo == 1)
            .map(|h| h.value.as_str())
    }

    fn download_url(&self) -> String {
        self.download_url
            .clone()
            .unwrap_or_else(|| cdn_url(self.id, &self.file_name))
    }
}

impl CurseForge {
    fn entry_fields(spec: &ModSpec) -> anyhow::Result<(String, String)> {
        match &spec.0 {
            ModEntry::Registry { id, version, .. } => Ok((id.clone(), version.clone())),
            ModEntry::Url { .. } => {
                anyhow::bail!("curseforge provider cannot resolve url entries")
            }
        }
    }
}

#[async_trait]
impl ModProvider for CurseForge {
    fn id(&self) -> &'static str {
        "curseforge"
    }

    async fn resolve(&self, spec: &ModSpec, env: &ResolveEnv) -> anyhow::Result<ResolvedMod> {
        let (id, version) = Self::entry_fields(spec)?;
        let datapack = env.loader_kind == "datapack";
        let project_id = self.resolve_project_id(&id, datapack).await?;
        let files = self.fetch_files(project_id, env).await?;

        let supports = |f: &CfFile| -> bool {
            let mc_ok =
                env.minecraft.is_empty() || f.game_versions.iter().any(|g| g == &env.minecraft);
            let loader_ok = datapack
                || mod_loader_type(&env.loader_kind).is_none()
                || f.game_versions
                    .iter()
                    .any(|g| g.eq_ignore_ascii_case(&env.loader_kind));
            mc_ok && loader_ok
        };

        let chosen = if version == "latest" {
            files.into_iter().find(supports)
        } else {
            files
                .into_iter()
                .find(|f| f.id.to_string() == version || f.display_name == version)
        }
        .ok_or_else(|| anyhow::anyhow!("no matching curseforge file for {id} {version}"))?;

        if !supports(&chosen) {
            anyhow::bail!(
                "curseforge file {} does not support minecraft {} on {}",
                chosen.display_name,
                env.minecraft,
                env.loader_kind
            );
        }

        let url = chosen.download_url();
        let bytes = crate::download::fetch_bytes(&self.client, &url).await?;

        if let Some(expected_sha1) = chosen.sha1() {
            use sha1::Digest;
            let mut h = sha1::Sha1::new();
            h.update(&bytes);
            let got = hex::encode(h.finalize());
            if !got.eq_ignore_ascii_case(expected_sha1) {
                anyhow::bail!(
                    "curseforge sha1 mismatch for {}: api says {expected_sha1}, got {got}",
                    chosen.file_name
                );
            }
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
            provider: "curseforge".into(),
            version: chosen.id.to_string(),
            filename: chosen.file_name,
            url,
            sha256,
        })
    }

    async fn list_versions(
        &self,
        spec: &ModSpec,
        env: &ResolveEnv,
    ) -> anyhow::Result<Vec<AvailableVersion>> {
        let (id, _) = Self::entry_fields(spec)?;
        let datapack = env.loader_kind == "datapack";
        let project_id = self.resolve_project_id(&id, datapack).await?;
        let files = self.fetch_files(project_id, env).await?;
        Ok(files
            .into_iter()
            .map(|f| {
                let (mc, mut loaders) = split_game_versions(&f.game_versions);
                if datapack {
                    loaders.push("datapack".to_string());
                }
                AvailableVersion {
                    version_number: f.id.to_string(),
                    game_versions: mc,
                    loaders,
                    date_published: f.file_date,
                }
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cdn_url_pads_file_id() {
        assert_eq!(
            cdn_url(6001, "jei-1.0.jar"),
            "https://edge.forgecdn.net/files/6/001/jei-1.0.jar"
        );
        assert_eq!(
            cdn_url(3940240, "mod name.jar"),
            "https://edge.forgecdn.net/files/3940/240/mod%20name.jar"
        );
    }

    #[test]
    fn loader_type_mapping() {
        assert_eq!(mod_loader_type("fabric"), Some(4));
        assert_eq!(mod_loader_type("neoforge"), Some(6));
        assert_eq!(mod_loader_type("vanilla"), None);
        assert_eq!(mod_loader_type("datapack"), None);
    }

    #[test]
    fn splits_loader_names_from_mc_versions() {
        let raw = vec![
            "1.21.1".to_string(),
            "Fabric".to_string(),
            "1.21".to_string(),
        ];
        let (mc, loaders) = split_game_versions(&raw);
        assert_eq!(mc, vec!["1.21.1", "1.21"]);
        assert_eq!(loaders, vec!["fabric"]);
    }
}
