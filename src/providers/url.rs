use async_trait::async_trait;
use crate::yml::ModEntry;
use crate::{ModProvider, ModSpec, ResolveEnv, ResolvedMod};

pub struct UrlProvider;

impl Default for UrlProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl UrlProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ModProvider for UrlProvider {
    fn id(&self) -> &'static str {
        "url"
    }

    async fn resolve(&self, spec: &ModSpec, _env: &ResolveEnv) -> anyhow::Result<ResolvedMod> {
        let (url, sha256, filename) = match &spec.0 {
            ModEntry::Url { url, sha256, filename, .. } => {
                (url.clone(), sha256.clone(), filename.clone())
            }
            ModEntry::Registry { .. } => {
                anyhow::bail!("url provider cannot resolve registry entries")
            }
        };
        let filename = filename.unwrap_or_else(|| {
            url.rsplit('/')
                .next()
                .unwrap_or("mod.jar")
                .split('?')
                .next()
                .unwrap_or("mod.jar")
                .to_string()
        });
        let id = filename.trim_end_matches(".jar").to_string();
        Ok(ResolvedMod {
            id,
            provider: "url".into(),
            version: "pinned".into(),
            filename,
            url,
            sha256,
        })
    }
}
