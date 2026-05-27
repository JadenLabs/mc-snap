pub mod modrinth;
pub mod url;

use mc_snap_core::{AvailableVersion, ModProvider, ModSpec, ResolveEnv, ResolvedMod};
use mc_snap_core::yml::ModEntry;
use std::sync::Arc;

pub fn registry() -> Vec<Arc<dyn ModProvider>> {
    vec![
        Arc::new(modrinth::Modrinth::new()),
        Arc::new(url::UrlProvider::new()),
    ]
}

pub fn provider_id(entry: &ModEntry) -> &str {
    match entry {
        ModEntry::Registry { provider, .. } => provider,
        ModEntry::Url { provider, .. } => provider,
    }
}

pub async fn resolve_entry(
    entry: &ModEntry,
    env: &ResolveEnv,
) -> anyhow::Result<ResolvedMod> {
    let id = provider_id(entry);
    for p in registry() {
        if p.id() == id {
            return p.resolve(&ModSpec(entry.clone()), env).await;
        }
    }
    anyhow::bail!("unknown mod provider: {id}")
}

pub async fn list_versions_for_entry(
    entry: &ModEntry,
    env: &ResolveEnv,
) -> anyhow::Result<Vec<AvailableVersion>> {
    let id = provider_id(entry);
    for p in registry() {
        if p.id() == id {
            return p.list_versions(&ModSpec(entry.clone()), env).await;
        }
    }
    anyhow::bail!("unknown mod provider: {id}")
}
