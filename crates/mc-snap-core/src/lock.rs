use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Lock {
    pub schema: u32,
    pub yml_hash: String,
    pub loader: LockLoader,
    #[serde(default, rename = "mods")]
    pub mods: Vec<LockMod>,
    #[serde(default)]
    pub jdk: Option<LockJdk>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LockLoader {
    #[serde(rename = "type")]
    pub kind: String,
    pub minecraft: String,
    #[serde(default)]
    pub loader_version: Option<String>,
    #[serde(default)]
    pub installer_version: Option<String>,
    pub server_jar_url: String,
    pub server_jar_sha256: String,
    #[serde(default)]
    pub extra: Vec<LockArtifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LockMod {
    pub id: String,
    pub provider: String,
    pub version: String,
    pub filename: String,
    pub url: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LockArtifact {
    pub name: String,
    pub url: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LockJdk {
    pub major: u32,
    pub source: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub sha256: Option<String>,
}

impl Lock {
    pub fn from_path(p: &Path) -> anyhow::Result<Self> {
        let s = std::fs::read_to_string(p)?;
        Ok(toml::from_str(&s)?)
    }

    pub fn write(&self, p: &Path) -> anyhow::Result<()> {
        let s = toml::to_string_pretty(self)?;
        // Write atomically via tmp+rename so a crash mid-write can't leave a
        // truncated lockfile that subsequent runs fail to parse.
        let tmp = p.with_extension("lock.tmp");
        std::fs::write(&tmp, s)?;
        std::fs::rename(&tmp, p)?;
        Ok(())
    }

    pub fn content_hash(&self) -> anyhow::Result<String> {
        use sha2::{Digest, Sha256};
        let s = toml::to_string_pretty(self)?;
        let mut hasher = Sha256::new();
        hasher.update(s.as_bytes());
        Ok(hex::encode(hasher.finalize()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let lock = Lock {
            schema: 1,
            yml_hash: "a".repeat(64),
            loader: LockLoader {
                kind: "fabric".into(),
                minecraft: "26.1.2".into(),
                loader_version: Some("0.16.9".into()),
                installer_version: Some("1.0.1".into()),
                server_jar_url: "https://example.com/server.jar".into(),
                server_jar_sha256: "b".repeat(64),
                extra: vec![],
            },
            mods: vec![LockMod {
                id: "fabric-api".into(),
                provider: "modrinth".into(),
                version: "0.110.0".into(),
                filename: "fabric-api.jar".into(),
                url: "https://cdn.modrinth.com/x.jar".into(),
                sha256: "c".repeat(64),
            }],
            jdk: None,
        };
        let s = toml::to_string_pretty(&lock).unwrap();
        let parsed: Lock = toml::from_str(&s).unwrap();
        assert_eq!(lock, parsed);
    }
}
