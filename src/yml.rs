use serde::{Deserialize, Serialize};
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

fn validate_location(loc: &str) -> anyhow::Result<()> {
    let trimmed = loc.trim();
    if trimmed.is_empty() || trimmed == "." {
        return Ok(());
    }
    let p = Path::new(trimmed);
    if p.is_absolute() {
        anyhow::bail!("server.location must be a relative path, got {trimmed}");
    }
    for comp in p.components() {
        use std::path::Component;
        match comp {
            Component::ParentDir => {
                anyhow::bail!("server.location must not contain `..`")
            }
            Component::Prefix(_) | Component::RootDir => {
                anyhow::bail!("server.location must be a relative path")
            }
            _ => {}
        }
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
            if let ModEntry::Url { sha256, .. } = entry {
                if sha256.len() != 64 || !sha256.chars().all(|c| c.is_ascii_hexdigit()) {
                    anyhow::bail!("url mod entries require a 64-char hex sha256");
                }
            }
        }
        if let Some(loc) = &self.server.location {
            validate_location(loc)?;
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
        let bad = "schema: 2\nserver:\n  name: x\n  minecraft: 26.1.2\n  loader: { type: vanilla }\n";
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
