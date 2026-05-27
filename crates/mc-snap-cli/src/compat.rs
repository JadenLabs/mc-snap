use anyhow::Result;
use mc_snap_core::lock::Lock;
use mc_snap_core::yml::{ModEntry, Snap};
use mc_snap_core::{AvailableVersion, ResolveEnv};
use std::collections::HashSet;

/// Per-mod compatibility against a target Minecraft version.
#[derive(Debug, Clone)]
pub struct ModCompatReport {
    pub mod_id: String,
    pub current_version: String,
    pub status: CompatStatus,
}

#[derive(Debug, Clone)]
pub enum CompatStatus {
    /// Mod has a version compatible with the target MC. Carries the suggested version.
    Compatible { suggested_version: String },
    /// Mod is registry-backed but no compatible version exists at target MC.
    Incompatible,
    /// URL-pinned mod; compatibility cannot be inferred.
    Unknown,
    /// Provider error.
    Error(String),
}

/// Newer versions of a mod available at the current Minecraft version.
#[derive(Debug, Clone)]
pub struct NewerVersionsReport {
    pub mod_id: String,
    pub current_version: String,
    pub newer: Vec<String>,
}

fn mod_id(entry: &ModEntry) -> String {
    match entry {
        ModEntry::Registry { id, .. } => id.clone(),
        ModEntry::Url { url, .. } => url.clone(),
    }
}

fn current_version(entry: &ModEntry) -> String {
    match entry {
        ModEntry::Registry { version, .. } => version.clone(),
        ModEntry::Url { .. } => "url".to_string(),
    }
}

/// Check each mod against a target Minecraft version.
pub async fn check_against(snap: &Snap, target_mc: &str) -> Result<Vec<ModCompatReport>> {
    let env = ResolveEnv {
        minecraft: target_mc.to_string(),
        loader_kind: snap.server.loader.kind.clone(),
        loader_version: snap.server.loader.version.clone(),
    };
    let mut out = Vec::with_capacity(snap.mods.len());
    for entry in &snap.mods {
        let id = mod_id(entry);
        let cur = current_version(entry);
        let status = match entry {
            ModEntry::Url { .. } => CompatStatus::Unknown,
            ModEntry::Registry { .. } => {
                match mc_snap_providers::list_versions_for_entry(entry, &env).await {
                    Ok(versions) => {
                        let chosen = versions.into_iter().find(|v| {
                            v.game_versions.iter().any(|g| g == target_mc)
                                && v.loaders.iter().any(|l| l == &env.loader_kind)
                        });
                        match chosen {
                            Some(v) => CompatStatus::Compatible {
                                suggested_version: v.version_number,
                            },
                            None => CompatStatus::Incompatible,
                        }
                    }
                    Err(e) => CompatStatus::Error(e.to_string()),
                }
            }
        };
        out.push(ModCompatReport {
            mod_id: id,
            current_version: cur,
            status,
        });
    }
    Ok(out)
}

/// For each registry mod, return all `game_versions` it has at least one release for
/// on the current loader. URL mods are ignored.
pub async fn collect_supported_mc_versions(
    snap: &Snap,
) -> Result<Vec<(String, HashSet<String>)>> {
    let env = ResolveEnv {
        minecraft: String::new(),
        loader_kind: snap.server.loader.kind.clone(),
        loader_version: snap.server.loader.version.clone(),
    };
    let mut out = Vec::new();
    for entry in &snap.mods {
        if matches!(entry, ModEntry::Url { .. }) {
            continue;
        }
        let id = mod_id(entry);
        let versions = mc_snap_providers::list_versions_for_entry(entry, &env).await?;
        let supported: HashSet<String> = versions
            .into_iter()
            .filter(|v| v.loaders.iter().any(|l| l == &env.loader_kind))
            .flat_map(|v| v.game_versions.into_iter())
            .collect();
        out.push((id, supported));
    }
    Ok(out)
}

/// Find the newest Minecraft version supported by *every* registry mod (intersection),
/// preferring versions newer than `current_mc`.
pub fn newest_common_mc(
    per_mod: &[(String, HashSet<String>)],
    current_mc: &str,
) -> Option<String> {
    if per_mod.is_empty() {
        return None;
    }
    let mut intersection: HashSet<String> = per_mod[0].1.clone();
    for (_, set) in &per_mod[1..] {
        intersection.retain(|v| set.contains(v));
    }
    let mut candidates: Vec<String> = intersection.into_iter().collect();
    candidates.sort_by(|a, b| compare_mc_version(b, a));
    candidates
        .into_iter()
        .find(|v| compare_mc_version(v, current_mc) == std::cmp::Ordering::Greater)
}

/// For each registry mod, list versions available on the current MC newer than the
/// locked version. "Newer" is determined by the order Modrinth returns versions
/// (newest first), so anything appearing before the locked version is newer.
pub async fn find_newer_versions(
    snap: &Snap,
    lock: &Lock,
) -> Result<Vec<NewerVersionsReport>> {
    let env = ResolveEnv {
        minecraft: snap.server.minecraft.clone(),
        loader_kind: snap.server.loader.kind.clone(),
        loader_version: snap.server.loader.version.clone(),
    };
    let mut out = Vec::new();
    for entry in &snap.mods {
        let ModEntry::Registry { id, .. } = entry else {
            continue;
        };
        let locked = lock
            .mods
            .iter()
            .find(|m| &m.id == id)
            .map(|m| m.version.clone())
            .unwrap_or_else(|| "?".to_string());
        let versions = mc_snap_providers::list_versions_for_entry(entry, &env).await?;
        let newer = collect_newer(&versions, &locked);
        out.push(NewerVersionsReport {
            mod_id: id.clone(),
            current_version: locked,
            newer,
        });
    }
    Ok(out)
}

fn collect_newer(versions: &[AvailableVersion], locked: &str) -> Vec<String> {
    let mut out = Vec::new();
    for v in versions {
        if v.version_number == locked {
            break;
        }
        out.push(v.version_number.clone());
    }
    out
}

/// Compare two dot-separated numeric version strings (e.g. "26.1.10" vs "26.1.2").
/// Pre-release suffixes like `-rc.1`, `-pre1`, `-beta` are split off and treated
/// as LOWER than the bare release per semver convention. Missing segments are 0.
pub fn compare_mc_version(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    let (core_a, pre_a) = split_pre_release(a);
    let (core_b, pre_b) = split_pre_release(b);

    let parts_a: Vec<&str> = core_a.split('.').collect();
    let parts_b: Vec<&str> = core_b.split('.').collect();
    let n = parts_a.len().max(parts_b.len());
    for i in 0..n {
        let pa = parts_a.get(i).copied().unwrap_or("0");
        let pb = parts_b.get(i).copied().unwrap_or("0");
        match (pa.parse::<u64>(), pb.parse::<u64>()) {
            (Ok(x), Ok(y)) => match x.cmp(&y) {
                Ordering::Equal => continue,
                o => return o,
            },
            _ => match pa.cmp(pb) {
                Ordering::Equal => continue,
                o => return o,
            },
        }
    }

    // Cores equal — fall back to pre-release comparison. A version with NO
    // pre-release outranks one that has one (1.0 > 1.0-rc).
    match (pre_a, pre_b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => x.cmp(&y),
    }
}

/// Split "26.1.0-rc.1" into ("26.1.0", Some("rc.1")). Mojang's snapshots use
/// "1.20-pre1" style and we accept either separator. We don't strip "+build"
/// metadata because Modrinth versions like "0.140.0+26.1.2" carry MC-version
/// info there and we want that to participate in ordering.
fn split_pre_release(s: &str) -> (&str, Option<&str>) {
    if let Some(idx) = s.find('-') {
        (&s[..idx], Some(&s[idx + 1..]))
    } else {
        (s, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions_numerically() {
        assert_eq!(
            compare_mc_version("26.1.10", "26.1.2"),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            compare_mc_version("26.1.2", "26.1.2"),
            std::cmp::Ordering::Equal
        );
        assert_eq!(
            compare_mc_version("26.1", "26.1.2"),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn release_outranks_pre_release() {
        assert_eq!(
            compare_mc_version("26.1.0", "26.1.0-rc.1"),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            compare_mc_version("26.1.0-rc.1", "26.1.0"),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            compare_mc_version("26.1.0-rc.1", "26.1.0-rc.2"),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn newest_common_picks_highest_in_intersection() {
        let a: HashSet<String> = ["26.1.2", "26.1.3", "26.1.4"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let b: HashSet<String> = ["26.1.2", "26.1.3"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let r = newest_common_mc(
            &[("a".into(), a), ("b".into(), b)],
            "26.1.2",
        );
        assert_eq!(r, Some("26.1.3".into()));
    }

    #[test]
    fn collect_newer_stops_at_locked() {
        let mk = |v: &str| AvailableVersion {
            version_number: v.into(),
            game_versions: vec![],
            loaders: vec![],
            date_published: None,
        };
        let versions = vec![mk("0.140.0"), mk("0.139.0"), mk("0.138.0")];
        let newer = collect_newer(&versions, "0.139.0");
        assert_eq!(newer, vec!["0.140.0"]);
    }
}
