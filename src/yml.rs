use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

fn de_scalar_as_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};
    use std::fmt;

    struct V;
    impl<'de> Visitor<'de> for V {
        type Value = String;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a string, number, or boolean")
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<String, E> {
            Ok(v.to_string())
        }
        fn visit_string<E: de::Error>(self, v: String) -> Result<String, E> {
            Ok(v)
        }
        fn visit_i64<E: de::Error>(self, v: i64) -> Result<String, E> {
            Ok(v.to_string())
        }
        fn visit_u64<E: de::Error>(self, v: u64) -> Result<String, E> {
            Ok(v.to_string())
        }
        fn visit_f64<E: de::Error>(self, v: f64) -> Result<String, E> {
            Ok(format!("{:?}", v))
        }
        fn visit_bool<E: de::Error>(self, v: bool) -> Result<String, E> {
            Ok(v.to_string())
        }
    }
    deserializer.deserialize_any(V)
}

fn de_opt_scalar_as_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};
    use std::fmt;

    struct V;
    impl<'de> Visitor<'de> for V {
        type Value = Option<String>;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("an optional string, number, or boolean")
        }
        fn visit_none<E: de::Error>(self) -> Result<Option<String>, E> {
            Ok(None)
        }
        fn visit_unit<E: de::Error>(self) -> Result<Option<String>, E> {
            Ok(None)
        }
        fn visit_some<D2: serde::Deserializer<'de>>(
            self,
            d: D2,
        ) -> Result<Option<String>, D2::Error> {
            de_scalar_as_string(d).map(Some)
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<Option<String>, E> {
            Ok(Some(v.to_string()))
        }
        fn visit_string<E: de::Error>(self, v: String) -> Result<Option<String>, E> {
            Ok(Some(v))
        }
        fn visit_i64<E: de::Error>(self, v: i64) -> Result<Option<String>, E> {
            Ok(Some(v.to_string()))
        }
        fn visit_u64<E: de::Error>(self, v: u64) -> Result<Option<String>, E> {
            Ok(Some(v.to_string()))
        }
        fn visit_f64<E: de::Error>(self, v: f64) -> Result<Option<String>, E> {
            Ok(Some(format!("{:?}", v)))
        }
        fn visit_bool<E: de::Error>(self, v: bool) -> Result<Option<String>, E> {
            Ok(Some(v.to_string()))
        }
    }
    deserializer.deserialize_any(V)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Snap {
    pub schema: u32,
    #[serde(default)]
    pub eula: bool,
    pub server: Server,
    #[serde(default)]
    pub runtime: Runtime,
    #[serde(default)]
    pub mods: Vec<ModEntry>,
    #[serde(default)]
    pub datapacks: Vec<DatapackEntry>,
    #[serde(default)]
    pub config: ConfigSection,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Server {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(deserialize_with = "de_scalar_as_string")]
    pub minecraft: String,
    pub loader: Loader,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Loader {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, deserialize_with = "de_opt_scalar_as_string")]
    pub version: Option<String>,
    #[serde(default, deserialize_with = "de_opt_scalar_as_string")]
    pub installer: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Runtime {
    #[serde(default)]
    pub java: Option<u32>,
    #[serde(default)]
    pub memory: Option<String>,
    #[serde(default)]
    pub flags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ModEntry {
    Registry {
        id: String,
        provider: String,
        #[serde(default = "default_version", deserialize_with = "de_scalar_as_string")]
        version: String,
    },
    Url {
        url: String,
        provider: String,
        sha256: String,
        #[serde(default)]
        filename: Option<String>,
    },
}

fn default_version() -> String {
    "latest".to_string()
}

/// A datapack entry. Datapacks are zip archives installed into the world's
/// `datapacks/` directory. Mirrors [`ModEntry`]'s untagged layout: variants are
/// disambiguated by which fields are present (`packs` -> VanillaTweaks,
/// `url`+`sha256` -> Url, `id` -> Registry). Registry covers both Modrinth and
/// CurseForge via the `provider` field.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum DatapackEntry {
    /// vanillatweaks.net packs, generated on demand from a category -> pack-name map.
    VanillaTweaks {
        provider: String,
        packs: BTreeMap<String, Vec<String>>,
        #[serde(default, deserialize_with = "de_opt_scalar_as_string")]
        version: Option<String>,
    },
    /// A direct download URL with a pinned sha256.
    Url {
        url: String,
        provider: String,
        sha256: String,
        #[serde(default)]
        filename: Option<String>,
    },
    /// A modrinth or curseforge project, resolved against the datapack loader.
    Registry {
        id: String,
        provider: String,
        #[serde(default = "default_version", deserialize_with = "de_scalar_as_string")]
        version: String,
    },
}

impl DatapackEntry {
    pub fn provider(&self) -> &str {
        match self {
            DatapackEntry::VanillaTweaks { provider, .. } => provider,
            DatapackEntry::Url { provider, .. } => provider,
            DatapackEntry::Registry { provider, .. } => provider,
        }
    }

    /// A short human label used in logs and progress output.
    pub fn label(&self) -> String {
        match self {
            DatapackEntry::VanillaTweaks { packs, .. } => {
                let count: usize = packs.values().map(|v| v.len()).sum();
                format!("vanillatweaks ({count} packs)")
            }
            DatapackEntry::Url { url, .. } => url.clone(),
            DatapackEntry::Registry { id, version, .. } => format!("{id} {version}"),
        }
    }
}

fn validate_location(loc: &str) -> anyhow::Result<()> {
    let trimmed = loc.trim();
    if trimmed.is_empty() || trimmed == "." {
        return Ok(());
    }
    validate_rel_path("server.location", trimmed)
}

/// Reject absolute paths and `..` components. Used for every yml field that is
/// later joined onto the project root or server dir; a shared bundle's yml is
/// untrusted input, so none of these may escape the project.
fn validate_rel_path(field: &str, raw: &str) -> anyhow::Result<()> {
    let p = Path::new(raw);
    if p.is_absolute() {
        anyhow::bail!("{field} must be a relative path, got {raw}");
    }
    for comp in p.components() {
        use std::path::Component;
        match comp {
            Component::ParentDir => {
                anyhow::bail!("{field} must not contain `..`: {raw}")
            }
            Component::Prefix(_) | Component::RootDir => {
                anyhow::bail!("{field} must be a relative path, got {raw}")
            }
            _ => {}
        }
    }
    Ok(())
}

/// A downloaded artifact's on-disk name must be a single plain filename; it is
/// joined into `mods/` or `<world>/datapacks/`, so separators or `..` would let
/// a hostile lockfile or registry response write outside the server dir.
pub fn validate_artifact_filename(context: &str, name: &str) -> anyhow::Result<()> {
    if name.trim().is_empty() {
        anyhow::bail!("{context}: filename must not be empty");
    }
    if name.contains('/') || name.contains('\\') {
        anyhow::bail!("{context}: filename must not contain path separators, got {name:?}");
    }
    if name == "." || name == ".." {
        anyhow::bail!("{context}: filename must not be a dot path, got {name:?}");
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ConfigSection {
    #[serde(rename = "server.properties", default)]
    pub server_properties: serde_yml::Mapping,
    #[serde(default)]
    pub files: Vec<FileRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileRef {
    pub src: String,
    pub dst: String,
}

impl Snap {
    pub fn from_str(s: &str) -> anyhow::Result<Self> {
        let snap: Snap = serde_yml::from_str(s)?;
        snap.validate()?;
        Ok(snap)
    }

    pub fn from_path(p: &Path) -> anyhow::Result<Self> {
        let s = std::fs::read_to_string(p)?;
        Self::from_str(&s)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.schema != 1 {
            anyhow::bail!("unsupported schema version {}; expected 1", self.schema);
        }
        if self.server.name.trim().is_empty() {
            anyhow::bail!("server.name must not be empty");
        }
        if self.server.minecraft.trim().is_empty() {
            anyhow::bail!("server.minecraft must not be empty");
        }
        for entry in &self.mods {
            if let ModEntry::Url {
                sha256, filename, ..
            } = entry
            {
                if sha256.len() != 64 || !sha256.chars().all(|c| c.is_ascii_hexdigit()) {
                    anyhow::bail!("url mod entries require a 64-char hex sha256");
                }
                if let Some(f) = filename {
                    validate_artifact_filename("url mod entry", f)?;
                }
            }
        }
        for dp in &self.datapacks {
            match dp {
                DatapackEntry::Url { sha256, .. } => {
                    if sha256.len() != 64 || !sha256.chars().all(|c| c.is_ascii_hexdigit()) {
                        anyhow::bail!("url datapack entries require a 64-char hex sha256");
                    }
                }
                DatapackEntry::VanillaTweaks {
                    packs, provider, ..
                } => {
                    if provider != "vanillatweaks" {
                        anyhow::bail!(
                            "datapack with `packs` must use provider: vanillatweaks, got {provider}"
                        );
                    }
                    if packs.is_empty() || packs.values().all(|v| v.is_empty()) {
                        anyhow::bail!("vanillatweaks datapack must list at least one pack");
                    }
                }
                DatapackEntry::Registry { provider, id, .. } => {
                    if provider != "modrinth" && provider != "curseforge" {
                        anyhow::bail!(
                            "datapack {id} has unsupported provider {provider} (use modrinth or curseforge)"
                        );
                    }
                }
            }
        }
        if let Some(loc) = &self.server.location {
            validate_location(loc)?;
        }
        // config.files paths are joined onto the project root / server dir at
        // install time; a bundle's yml must not be able to read or write
        // outside the project.
        for f in &self.config.files {
            validate_rel_path("config.files src", &f.src)?;
            validate_rel_path("config.files dst", &f.dst)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
schema: 1
eula: true
server:
  name: grimwald
  description: the grimwald smp
  minecraft: 26.1.2
  loader:
    type: fabric
    version: 0.16.9
runtime:
  java: 26
  memory: 4G
  flags:
    - -XX:+UseG1GC
mods:
  - id: fabric-api
    provider: modrinth
    version: "0.140.0+26.1.2"
  - url: https://example.com/mymod.jar
    provider: url
    sha256: 0000000000000000000000000000000000000000000000000000000000000000
config:
  server.properties:
    motd: hi
    max-players: 20
"#;

    #[test]
    fn parses_sample() {
        let snap = Snap::from_str(SAMPLE).unwrap();
        assert_eq!(snap.server.name, "grimwald");
        assert_eq!(snap.server.loader.kind, "fabric");
        assert_eq!(snap.mods.len(), 2);
        assert_eq!(snap.runtime.java, Some(26));
    }

    #[test]
    fn rejects_bad_sha() {
        let bad = r#"
schema: 1
server:
  name: x
  minecraft: 26.1.2
  loader: { type: vanilla }
mods:
  - url: https://example.com/x.jar
    provider: url
    sha256: deadbeef
"#;
        assert!(Snap::from_str(bad).is_err());
    }

    #[test]
    fn rejects_wrong_schema() {
        let bad =
            "schema: 2\nserver:\n  name: x\n  minecraft: 26.1.2\n  loader: { type: vanilla }\n";
        assert!(Snap::from_str(bad).is_err());
    }

    #[test]
    fn accepts_relative_location() {
        let yml = "schema: 1\nserver:\n  name: x\n  minecraft: 26.1.2\n  location: server\n  loader: { type: vanilla }\n";
        let snap = Snap::from_str(yml).unwrap();
        assert_eq!(snap.server.location.as_deref(), Some("server"));
    }

    #[test]
    fn rejects_absolute_location() {
        let yml = "schema: 1\nserver:\n  name: x\n  minecraft: 26.1.2\n  location: /etc\n  loader: { type: vanilla }\n";
        assert!(Snap::from_str(yml).is_err());
    }

    #[test]
    fn rejects_parent_traversal_location() {
        let yml = "schema: 1\nserver:\n  name: x\n  minecraft: 26.1.2\n  location: ../escape\n  loader: { type: vanilla }\n";
        assert!(Snap::from_str(yml).is_err());
    }

    #[test]
    fn missing_location_defaults_to_none() {
        let snap = Snap::from_str(SAMPLE).unwrap();
        assert!(snap.server.location.is_none());
    }

    #[test]
    fn parses_datapack_entries() {
        let yml = r#"
schema: 1
server:
  name: x
  minecraft: 26.1.2
  loader: { type: fabric }
datapacks:
  - id: terralith
    provider: modrinth
    version: latest
  - id: "12345"
    provider: curseforge
    version: 67890
  - provider: vanillatweaks
    version: "1.21"
    packs:
      survival:
        - graves
        - multiplayer-sleep
      mobs:
        - armor-statues
  - url: https://example.com/pack.zip
    provider: url
    sha256: 0000000000000000000000000000000000000000000000000000000000000000
    filename: pack.zip
"#;
        let snap = Snap::from_str(yml).unwrap();
        assert_eq!(snap.datapacks.len(), 4);
        match &snap.datapacks[0] {
            DatapackEntry::Registry {
                id,
                provider,
                version,
            } => {
                assert_eq!(id, "terralith");
                assert_eq!(provider, "modrinth");
                assert_eq!(version, "latest");
            }
            _ => panic!("expected Registry"),
        }
        match &snap.datapacks[1] {
            DatapackEntry::Registry {
                provider, version, ..
            } => {
                assert_eq!(provider, "curseforge");
                assert_eq!(version, "67890");
            }
            _ => panic!("expected Registry"),
        }
        match &snap.datapacks[2] {
            DatapackEntry::VanillaTweaks {
                provider,
                packs,
                version,
            } => {
                assert_eq!(provider, "vanillatweaks");
                assert_eq!(version.as_deref(), Some("1.21"));
                assert_eq!(packs["survival"], vec!["graves", "multiplayer-sleep"]);
                assert_eq!(packs["mobs"], vec!["armor-statues"]);
            }
            _ => panic!("expected VanillaTweaks"),
        }
        match &snap.datapacks[3] {
            DatapackEntry::Url {
                provider, filename, ..
            } => {
                assert_eq!(provider, "url");
                assert_eq!(filename.as_deref(), Some("pack.zip"));
            }
            _ => panic!("expected Url"),
        }
    }

    #[test]
    fn rejects_bad_datapack_sha() {
        let bad = r#"
schema: 1
server:
  name: x
  minecraft: 26.1.2
  loader: { type: vanilla }
datapacks:
  - url: https://example.com/x.zip
    provider: url
    sha256: deadbeef
"#;
        assert!(Snap::from_str(bad).is_err());
    }

    #[test]
    fn rejects_empty_vanillatweaks_packs() {
        let bad = r#"
schema: 1
server:
  name: x
  minecraft: 26.1.2
  loader: { type: vanilla }
datapacks:
  - provider: vanillatweaks
    packs: {}
"#;
        assert!(Snap::from_str(bad).is_err());
    }

    #[test]
    fn missing_datapacks_defaults_empty() {
        let snap = Snap::from_str(SAMPLE).unwrap();
        assert!(snap.datapacks.is_empty());
    }

    #[test]
    fn rejects_traversal_in_config_files() {
        let bad_dst = r#"
schema: 1
server:
  name: x
  minecraft: 26.1.2
  loader: { type: vanilla }
config:
  files:
    - { src: configs/a.toml, dst: ../../escape.toml }
"#;
        assert!(Snap::from_str(bad_dst).is_err());

        let bad_src = r#"
schema: 1
server:
  name: x
  minecraft: 26.1.2
  loader: { type: vanilla }
config:
  files:
    - { src: /etc/passwd, dst: config/a.toml }
"#;
        assert!(Snap::from_str(bad_src).is_err());
    }

    #[test]
    fn rejects_separator_in_url_mod_filename() {
        let bad = r#"
schema: 1
server:
  name: x
  minecraft: 26.1.2
  loader: { type: vanilla }
mods:
  - url: https://example.com/x.jar
    provider: url
    sha256: 0000000000000000000000000000000000000000000000000000000000000000
    filename: ../evil.jar
"#;
        assert!(Snap::from_str(bad).is_err());
    }

    #[test]
    fn validate_artifact_filename_rules() {
        assert!(validate_artifact_filename("t", "mod.jar").is_ok());
        assert!(validate_artifact_filename("t", "a b (1.2).jar").is_ok());
        assert!(validate_artifact_filename("t", "").is_err());
        assert!(validate_artifact_filename("t", "  ").is_err());
        assert!(validate_artifact_filename("t", "..").is_err());
        assert!(validate_artifact_filename("t", "a/b.jar").is_err());
        assert!(validate_artifact_filename("t", "a\\b.jar").is_err());
    }

    #[test]
    fn coerces_numeric_scalars_to_string() {
        let yml = r#"
schema: 1
server:
  name: x
  minecraft: 1.21
  loader:
    type: fabric
    version: 0.16
mods:
  - id: better-multiplayer-sleep
    provider: modrinth
    version: 1.0
  - id: chunky
    provider: modrinth
    version: 5
  - id: example
    provider: modrinth
    version: true
"#;
        let snap = Snap::from_str(yml).unwrap();
        assert_eq!(snap.server.minecraft, "1.21");
        assert_eq!(snap.server.loader.version.as_deref(), Some("0.16"));
        match &snap.mods[0] {
            ModEntry::Registry { version, .. } => assert_eq!(version, "1.0"),
            _ => panic!("expected Registry"),
        }
        match &snap.mods[1] {
            ModEntry::Registry { version, .. } => assert_eq!(version, "5"),
            _ => panic!("expected Registry"),
        }
        match &snap.mods[2] {
            ModEntry::Registry { version, .. } => assert_eq!(version, "true"),
            _ => panic!("expected Registry"),
        }
    }
}
