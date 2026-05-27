use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InstallState {
    pub applied_lock_hash: Option<String>,
    pub installed_at: Option<String>,
}

impl InstallState {
    pub fn load(p: &Path) -> anyhow::Result<Self> {
        if !p.exists() {
            return Ok(Self::default());
        }
        let s = std::fs::read_to_string(p)?;
        Ok(serde_json::from_str(&s)?)
    }

    pub fn save(&self, p: &Path) -> anyhow::Result<()> {
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(p, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}
