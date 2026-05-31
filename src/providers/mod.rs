pub mod curseforge;
pub mod modrinth;
pub mod url;
pub mod vanillatweaks;

use crate::yml::{DatapackEntry, ModEntry};
use crate::{AvailableVersion, ModProvider, ModSpec, ResolveEnv, ResolvedMod};
use std::sync::Arc;

pub fn registry() -> Vec<Arc<dyn ModProvider>> {
    vec![
        Arc::new(modrinth::Modrinth::new()),
        Arc::new(curseforge::CurseForge::new()),
        Arc::new(url::UrlProvider::new()),
    ]
}

pub fn provider_id(entry: &ModEntry) -> &str {
    match entry {
        ModEntry::Registry { provider, .. } => provider,
        ModEntry::Url { provider, .. } => provider,
    }
}

pub async fn resolve_entry(entry: &ModEntry, env: &ResolveEnv) -> anyhow::Result<ResolvedMod> {
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

/// Resolve a datapack entry into a downloadable artifact. Registry datapacks
/// reuse the Modrinth/CurseForge providers under the synthetic `datapack` loader;
/// Url datapacks reuse the url provider; VanillaTweaks bundles are generated on
/// the fly. `env.minecraft` is used as the game version when not pinned.
pub async fn resolve_datapack(
    entry: &DatapackEntry,
    env: &ResolveEnv,
) -> anyhow::Result<ResolvedMod> {
    match entry {
        DatapackEntry::Registry {
            id,
            provider,
            version,
        } => {
            let mod_entry = ModEntry::Registry {
                id: id.clone(),
                provider: provider.clone(),
                version: version.clone(),
            };
            let dp_env = ResolveEnv {
                minecraft: env.minecraft.clone(),
                loader_kind: "datapack".into(),
                loader_version: None,
            };
            resolve_entry(&mod_entry, &dp_env).await
        }
        DatapackEntry::Url {
            url,
            sha256,
            filename,
            ..
        } => {
            let mod_entry = ModEntry::Url {
                url: url.clone(),
                provider: "url".into(),
                sha256: sha256.clone(),
                filename: filename.clone(),
            };
            resolve_entry(&mod_entry, env).await
        }
        DatapackEntry::VanillaTweaks { packs, version, .. } => {
            let mc = version.clone().unwrap_or_else(|| env.minecraft.clone());
            vanillatweaks::VanillaTweaks::new()
                .resolve(&mc, packs)
                .await
        }
    }
}
