use crate::cache::{sha256_hex, ContentCache};
use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use std::path::PathBuf;
use std::time::Duration;

/// Hard cap for any single download. Server jars / mods / JDKs are well under this.
pub const MAX_DOWNLOAD_BYTES: u64 = 1024 * 1024 * 1024;

pub async fn fetch_bytes(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    fetch_bytes_capped(client, url, MAX_DOWNLOAD_BYTES).await
}

/// Stream the response and abort if it exceeds `max_bytes`. Avoids OOM on a hostile
/// or compromised endpoint that returns gigabytes.
pub async fn fetch_bytes_capped(
    client: &reqwest::Client,
    url: &str,
    max_bytes: u64,
) -> Result<Vec<u8>> {
    let resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?
        .error_for_status()
        .with_context(|| format!("status for {url}"))?;

    if let Some(len) = resp.content_length() {
        if len > max_bytes {
            bail!("{url} content-length {len} exceeds cap {max_bytes}");
        }
    }

    let mut stream = resp.bytes_stream();
    let mut buf = Vec::with_capacity(1024 * 64);
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.with_context(|| format!("stream chunk for {url}"))?;
        if (buf.len() as u64) + (chunk.len() as u64) > max_bytes {
            bail!("{url} exceeds size cap {max_bytes} mid-stream");
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

pub async fn fetch_into_cache(
    client: &reqwest::Client,
    cache: &ContentCache,
    url: &str,
    expected_sha256: &str,
) -> Result<PathBuf> {
    if cache.contains(expected_sha256) {
        // Re-verify the cached file once on hit. An interrupted prior write or disk
        // corruption otherwise gets served indefinitely.
        let path = cache.path_for(expected_sha256);
        let bytes = std::fs::read(&path)
            .with_context(|| format!("reading cache file {}", path.display()))?;
        let got = sha256_hex(&bytes);
        if got == expected_sha256 {
            return Ok(path);
        }
        tracing::warn!(
            "cache file {} has wrong sha256 (expected {}, got {}); refetching",
            path.display(),
            expected_sha256,
            got
        );
        let _ = std::fs::remove_file(&path);
    }
    let bytes = fetch_bytes(client, url).await?;
    let got = sha256_hex(&bytes);
    if got != expected_sha256 {
        bail!("sha256 mismatch for {url}: expected {expected_sha256}, got {got}");
    }
    cache.store(expected_sha256, &bytes)
}

pub fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(concat!("mc-snap/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::limited(5))
        .timeout(Duration::from_secs(120))
        .connect_timeout(Duration::from_secs(15))
        .build()?)
}
