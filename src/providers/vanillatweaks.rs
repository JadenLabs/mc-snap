use crate::ResolvedMod;
use serde::Deserialize;
use std::collections::BTreeMap;

const SITE: &str = "https://vanillatweaks.net";

/// VanillaTweaks datapack generator. Unlike Modrinth/CurseForge there is no
/// project to resolve: the user picks a set of packs grouped by category and the
/// site bundles them into a single zip on demand. We POST the selection, follow
/// the returned link to download the generated zip, then content-address it.
///
/// The generated download link is ephemeral (it points at a freshly built zip on
/// the VanillaTweaks side), so the locked `url` may stop working once the build is
/// garbage-collected. That is fine in practice: the content-addressed cache keeps
/// the zip by sha256, so reinstalls on the same machine never refetch.
pub struct VanillaTweaks {
    client: reqwest::Client,
    base: String,
}

impl Default for VanillaTweaks {
    fn default() -> Self {
        Self::new()
    }
}

impl VanillaTweaks {
    pub fn new() -> Self {
        Self {
            client: crate::download::http_client().expect("client"),
            base: SITE.to_string(),
        }
    }

    pub fn with_base(base: impl Into<String>) -> Self {
        Self {
            base: base.into(),
            ..Self::new()
        }
    }

    /// Generate and download a datapack bundle for `version` (a VanillaTweaks
    /// game version such as `1.21`) from the selected `packs`.
    pub async fn resolve(
        &self,
        version: &str,
        packs: &BTreeMap<String, Vec<String>>,
    ) -> anyhow::Result<ResolvedMod> {
        if packs.values().all(|v| v.is_empty()) {
            anyhow::bail!("vanillatweaks datapack selection is empty");
        }
        let packs_json = serde_json::to_string(packs)?;
        let gen_url = format!("{}/assets/server/zipdatapacks.php", self.base);
        let resp: ZipResponse = self
            .client
            .post(&gen_url)
            .form(&[("version", version), ("packs", packs_json.as_str())])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        if resp.status != "success" {
            anyhow::bail!(
                "vanillatweaks generation failed (status {}): {}",
                resp.status,
                resp.message.unwrap_or_default()
            );
        }
        let link = resp
            .link
            .ok_or_else(|| anyhow::anyhow!("vanillatweaks response missing download link"))?;
        let url = if link.starts_with("http") {
            link
        } else {
            format!("{}{}", self.base, link)
        };

        let bytes = crate::download::fetch_bytes(&self.client, &url).await?;
        let sha256 = crate::download::prime_cache(&bytes);

        let filename = url
            .rsplit('/')
            .next()
            .and_then(|s| s.split('?').next())
            .filter(|s| !s.is_empty())
            .unwrap_or("vanillatweaks-datapacks.zip")
            .to_string();

        Ok(ResolvedMod {
            id: "vanillatweaks".into(),
            provider: "vanillatweaks".into(),
            version: version.to_string(),
            filename,
            url,
            sha256,
        })
    }
}

#[derive(Debug, Deserialize)]
struct ZipResponse {
    status: String,
    #[serde(default)]
    link: Option<String>,
    #[serde(default)]
    message: Option<String>,
}
