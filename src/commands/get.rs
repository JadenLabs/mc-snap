use crate::paths::ProjectLayout;
use crate::proclock::ProjectLock;
use crate::yml::{ModEntry, Snap};
use crate::{AvailableVersion, ResolveEnv};
use anyhow::{Context, Result};

/// `mc-snap get <slug> [...] [--version V] [--provider modrinth] [--no-install]`
///
/// For each slug, look up the latest version on the registry that is compatible
/// with the snap's Minecraft + loader, add a `ModEntry::Registry` to the yml,
/// and run install (unless `--no-install` is set).
pub async fn run(
    slugs: Vec<String>,
    version: Option<String>,
    provider: Option<String>,
    no_install: bool,
) -> Result<()> {
    if slugs.is_empty() {
        anyhow::bail!("get: at least one mod slug is required");
    }
    if slugs.len() > 1 && version.is_some() {
        anyhow::bail!("--version can only be used with a single mod slug");
    }

    let layout = ProjectLayout::discover(&std::env::current_dir()?)?;
    let _guard = {
        std::fs::create_dir_all(layout.snap_dir())?;
        ProjectLock::acquire(&layout.lock_file())?
    };

    let mut snap = Snap::from_path(&layout.yml())?;
    let provider_id = provider.unwrap_or_else(|| "modrinth".to_string());

    let env = ResolveEnv {
        minecraft: snap.server.minecraft.clone(),
        loader_kind: snap.server.loader.kind.clone(),
        loader_version: snap.server.loader.version.clone(),
    };

    let mut added = 0usize;
    for slug in &slugs {
        if snap.mods.iter().any(|m| match m {
            ModEntry::Registry { id, .. } => id == slug,
            _ => false,
        }) {
            println!("skip {slug}: already in mods");
            continue;
        }

        let probe = ModEntry::Registry {
            id: slug.clone(),
            provider: provider_id.clone(),
            version: "latest".to_string(),
        };
        let versions = crate::providers::list_versions_for_entry(&probe, &env)
            .await
            .with_context(|| format!("looking up versions for {slug}"))?;

        let chosen = pick_version(&versions, version.as_deref(), &env)
            .with_context(|| format!("no compatible version for {slug}"))?;

        println!(
            "+ {slug} {ver} ({provider})",
            ver = chosen,
            provider = provider_id
        );
        snap.mods.push(ModEntry::Registry {
            id: slug.clone(),
            provider: provider_id.clone(),
            version: chosen,
        });
        added += 1;
    }

    if added == 0 {
        println!("nothing to add");
        return Ok(());
    }

    let rendered = crate::commands::init::render_yml(&snap);
    std::fs::write(layout.yml(), rendered).context("writing mc-snap.yml")?;
    println!("updated mc-snap.yml (+{added} mods)");

    if no_install {
        return Ok(());
    }

    // Re-run install to materialize. Drop our lock first since install acquires
    // its own; the file lock is reentrant only via the same guard.
    drop(_guard);
    crate::commands::install::run(crate::cache::LinkMode::default(), false).await
}

/// Pick a version from a provider listing. If `pinned` is Some, find an exact
/// match against version_number (or id). Otherwise, return the newest entry
/// that satisfies the env's MC + loader filters. The provider listing is
/// already filtered by query params, but we double-check in case the provider
/// returns extras.
fn pick_version(
    versions: &[AvailableVersion],
    pinned: Option<&str>,
    env: &ResolveEnv,
) -> Result<String> {
    let compatible = |v: &AvailableVersion| -> bool {
        let mc_ok = env.minecraft.is_empty() || v.game_versions.iter().any(|g| g == &env.minecraft);
        let loader_ok = v.loaders.iter().any(|l| l == &env.loader_kind);
        mc_ok && loader_ok
    };

    if let Some(want) = pinned {
        let hit = versions.iter().find(|v| v.version_number == want);
        let v = hit.ok_or_else(|| {
            anyhow::anyhow!(
                "version `{want}` not found on registry (loader={}, mc={})",
                env.loader_kind,
                env.minecraft
            )
        })?;
        if !compatible(v) {
            anyhow::bail!(
                "version `{want}` does not support loader={} mc={}",
                env.loader_kind,
                env.minecraft
            );
        }
        return Ok(v.version_number.clone());
    }

    versions
        .iter()
        .find(|v| compatible(v))
        .map(|v| v.version_number.clone())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no version supports loader={} mc={}",
                env.loader_kind,
                env.minecraft
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn av(ver: &str, mcs: &[&str], loaders: &[&str]) -> AvailableVersion {
        AvailableVersion {
            version_number: ver.to_string(),
            game_versions: mcs.iter().map(|s| s.to_string()).collect(),
            loaders: loaders.iter().map(|s| s.to_string()).collect(),
            date_published: None,
        }
    }

    fn env(mc: &str, loader: &str) -> ResolveEnv {
        ResolveEnv {
            minecraft: mc.to_string(),
            loader_kind: loader.to_string(),
            loader_version: None,
        }
    }

    #[test]
    fn picks_newest_compatible() {
        let versions = vec![
            av("0.140.0", &["26.1.2"], &["fabric"]),
            av("0.139.0", &["26.1.1"], &["fabric"]),
        ];
        let got = pick_version(&versions, None, &env("26.1.2", "fabric")).unwrap();
        assert_eq!(got, "0.140.0");
    }

    #[test]
    fn skips_incompatible_at_top() {
        let versions = vec![
            av("0.140.0", &["26.1.3"], &["fabric"]),
            av("0.139.0", &["26.1.2"], &["fabric"]),
        ];
        let got = pick_version(&versions, None, &env("26.1.2", "fabric")).unwrap();
        assert_eq!(got, "0.139.0");
    }

    #[test]
    fn skips_wrong_loader() {
        let versions = vec![
            av("0.140.0", &["26.1.2"], &["forge"]),
            av("0.139.0", &["26.1.2"], &["fabric"]),
        ];
        let got = pick_version(&versions, None, &env("26.1.2", "fabric")).unwrap();
        assert_eq!(got, "0.139.0");
    }

    #[test]
    fn errors_when_nothing_compatible() {
        let versions = vec![av("0.140.0", &["26.1.1"], &["forge"])];
        let err = pick_version(&versions, None, &env("26.1.2", "fabric")).unwrap_err();
        assert!(err.to_string().contains("no version supports"));
    }

    #[test]
    fn pinned_exact_match_returns_it() {
        let versions = vec![
            av("0.140.0", &["26.1.2"], &["fabric"]),
            av("0.139.0", &["26.1.2"], &["fabric"]),
        ];
        let got = pick_version(&versions, Some("0.139.0"), &env("26.1.2", "fabric")).unwrap();
        assert_eq!(got, "0.139.0");
    }

    #[test]
    fn pinned_incompatible_errors() {
        let versions = vec![av("0.140.0", &["26.1.1"], &["fabric"])];
        let err = pick_version(&versions, Some("0.140.0"), &env("26.1.2", "fabric")).unwrap_err();
        assert!(err.to_string().contains("does not support"));
    }

    #[test]
    fn pinned_missing_errors() {
        let versions = vec![av("0.140.0", &["26.1.2"], &["fabric"])];
        let err = pick_version(&versions, Some("9.9.9"), &env("26.1.2", "fabric")).unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn empty_mc_skips_mc_filter() {
        // env.minecraft = "" means caller doesn't want MC filtering applied.
        let versions = vec![av("0.140.0", &["whatever"], &["fabric"])];
        let got = pick_version(&versions, None, &env("", "fabric")).unwrap();
        assert_eq!(got, "0.140.0");
    }
}
