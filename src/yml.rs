use serde::{Deserialize, Serialize};
use std::path::Path;

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
    pub minecraft: String,
    pub loader: Loader,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Loader {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
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
        #[serde(default = "default_version")]
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
}
