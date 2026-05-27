pub mod fabric;
pub mod vanilla;

use crate::ServerLoader;
use std::sync::Arc;

pub fn for_kind(kind: &str) -> anyhow::Result<Arc<dyn ServerLoader>> {
    match kind {
        "vanilla" => Ok(Arc::new(vanilla::Vanilla::new())),
        "fabric" => Ok(Arc::new(fabric::Fabric::new())),
        other => anyhow::bail!("unknown loader: {other}"),
    }
}
