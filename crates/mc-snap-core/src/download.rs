use crate::cache::{sha256_hex, ContentCache};
use anyhow::{Context, Result};
use std::path::PathBuf;

pub async fn fetch_bytes(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?
        .error_for_status()
        .with_context(|| format!("status for {url}"))?;
    Ok(resp.bytes().await?.to_vec())
}

pub async fn fetch_into_cache(
    client: &reqwest::Client,
    cache: &ContentCache,
    url: &str,
    expected_sha256: &str,
) -> Result<PathBuf> {
    if cache.contains(expected_sha256) {
        return Ok(cache.path_for(expected_sha256));
    }
    let bytes = fetch_bytes(client, url).await?;
    let got = sha256_hex(&bytes);
    if got != expected_sha256 {
        anyhow::bail!(
            "sha256 mismatch for {url}: expected {expected_sha256}, got {got}"
        );
    }
    cache.store(expected_sha256, &bytes)
}

pub fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(concat!("mc-snap/", env!("CARGO_PKG_VERSION")))
        .build()?)
}
