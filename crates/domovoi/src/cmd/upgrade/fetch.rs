//! Talking to GitHub releases.

use super::is_release_tag;
use anyhow::Context;

/// The largest archive accepted. Release archives are a few tens of megabytes.
pub const MAX_ARCHIVE: usize = 256 * 1024 * 1024;
/// The largest checksum file accepted.
pub const MAX_CHECKSUM: usize = 4096;

/// Where releases are downloaded from: GitHub, or `DOMOVOI_RELEASE_BASE` as in
/// `install.sh`, which is how the command is tried against a local server.
pub fn release_base() -> String {
    std::env::var("DOMOVOI_RELEASE_BASE")
        .ok()
        .filter(|base| !base.is_empty())
        .unwrap_or_else(|| "https://github.com/RoastSlav/domovoi/releases".to_owned())
        .trim_end_matches('/')
        .to_owned()
}

/// An HTTP client with the user agent every Domovoi request carries.
pub fn client() -> anyhow::Result<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(300))
        .user_agent(concat!("domovoi/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("cannot create an HTTP client")
}

/// The tag of the latest release: where `<base>/latest` redirects to, as `install.sh`
/// reads it. The tag is checked, because it ends up in download URLs.
pub async fn latest_tag(client: &reqwest::Client, base: &str) -> anyhow::Result<String> {
    let url = format!("{base}/latest");
    let response = client
        .head(&url)
        .send()
        .await
        .map_err(|err| anyhow::anyhow!("cannot reach {url}: {}", err.without_url()))?;
    let tag = response
        .url()
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .unwrap_or_default()
        .to_owned();
    anyhow::ensure!(
        is_release_tag(&tag),
        "could not find the latest release at {url}; pass --version"
    );
    Ok(tag)
}

/// The body at `url`, refused if it is larger than `max` bytes or the answer is not a
/// success.
pub async fn download(client: &reqwest::Client, url: &str, max: usize) -> anyhow::Result<Vec<u8>> {
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|err| anyhow::anyhow!("cannot download {url}: {}", err.without_url()))?;
    anyhow::ensure!(
        response.status().is_success(),
        "{url} answered {}",
        response.status()
    );
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|err| anyhow::anyhow!("download of {url} failed: {}", err.without_url()))?
    {
        anyhow::ensure!(
            body.len() + chunk.len() <= max,
            "{url} is larger than the {max} bytes allowed"
        );
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}
