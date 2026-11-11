//! The Content-Security-Policy of every response. Scripts may come from the hub itself,
//! plus each inline script of the built `index.html`, allowed by its SHA-256 hash so the
//! policy never needs `'unsafe-inline'` and cannot drift from the page it protects.

use axum::http::HeaderValue;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use sha2::{Digest, Sha256};

/// Every directive of the policy except `script-src`.
pub const BASE_POLICY: &str = "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; frame-ancestors 'none'; base-uri 'none'";

/// The policy when no inline script is allowed: [`BASE_POLICY`] plus `script-src 'self'`.
pub const SCRIPTS_FROM_SELF_POLICY: &str = "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; frame-ancestors 'none'; base-uri 'none'; script-src 'self'";

/// The CSP source (`'sha256-<base64>'`) that allows exactly the inline script `body`.
#[must_use]
pub fn hash_source(body: &str) -> String {
    format!(
        "'sha256-{}'",
        STANDARD.encode(Sha256::digest(body.as_bytes()))
    )
}

/// The hash sources of every inline script in `html`: each `<script>` without a `src`
/// attribute, hashed over the exact text between its tag and `</script>`. Scripts inside
/// HTML comments are skipped.
#[must_use]
pub fn inline_script_hashes(html: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let mut hashes = Vec::new();
    let mut at = 0;
    while let Some(found) = lower[at..].find('<') {
        let start = at + found;
        if lower[start..].starts_with("<!--") {
            at = lower[start..]
                .find("-->")
                .map_or(lower.len(), |end| start + end + 3);
            continue;
        }
        at = start + 1;
        let Some(after_name) = lower[start..].strip_prefix("<script") else {
            continue;
        };
        if !after_name.starts_with(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/') {
            continue;
        }
        let Some((has_src, body_start)) = read_tag(html, start + "<script".len()) else {
            break;
        };
        let body_end = lower[body_start..]
            .find("</script")
            .map_or(lower.len(), |end| body_start + end);
        if !has_src {
            hashes.push(hash_source(&html[body_start..body_end]));
        }
        at = body_end;
    }
    hashes
}

/// Reads the attributes of an opening tag from `from`, just after the tag name. Returns
/// whether it has a `src` attribute and the offset just past its closing `>`.
fn read_tag(html: &str, from: usize) -> Option<(bool, usize)> {
    let bytes = html.as_bytes();
    let mut i = from;
    let mut has_src = false;
    loop {
        while bytes.get(i)?.is_ascii_whitespace() || *bytes.get(i)? == b'/' {
            i += 1;
        }
        if *bytes.get(i)? == b'>' {
            return Some((has_src, i + 1));
        }
        let name_start = i;
        while !matches!(bytes.get(i)?, b'=' | b'>' | b'/') && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        has_src |= html[name_start..i].eq_ignore_ascii_case("src");
        while bytes.get(i)?.is_ascii_whitespace() {
            i += 1;
        }
        if *bytes.get(i)? != b'=' {
            continue;
        }
        i += 1;
        while bytes.get(i)?.is_ascii_whitespace() {
            i += 1;
        }
        match *bytes.get(i)? {
            quote @ (b'"' | b'\'') => {
                i += 1;
                while *bytes.get(i)? != quote {
                    i += 1;
                }
                i += 1;
            }
            _ => {
                while !bytes.get(i)?.is_ascii_whitespace() && *bytes.get(i)? != b'>' {
                    i += 1;
                }
            }
        }
    }
}

/// The full policy for a page whose inline scripts are `hashes`: [`BASE_POLICY`] plus
/// `script-src 'self'` and one hash source per inline script.
#[must_use]
pub fn policy(hashes: &[String]) -> String {
    let mut policy = format!("{BASE_POLICY}; script-src 'self'");
    for hash in hashes {
        policy.push(' ');
        policy.push_str(hash);
    }
    policy
}

/// The policy header value for the embedded web UI, computed from its `index.html`. Falls
/// back to the policy without hashes when the UI is not embedded or the value is invalid.
#[must_use]
pub fn header_value() -> HeaderValue {
    #[cfg(feature = "embed-ui")]
    let hashes = {
        crate::spa::WebDist::get("index.html")
            .and_then(|file| String::from_utf8(file.data.into_owned()).ok())
            .map(|html| inline_script_hashes(&html))
            .unwrap_or_default()
    };
    #[cfg(not(feature = "embed-ui"))]
    let hashes: Vec<String> = Vec::new();
    HeaderValue::from_str(&policy(&hashes))
        .unwrap_or_else(|_| HeaderValue::from_static(SCRIPTS_FROM_SELF_POLICY))
}
