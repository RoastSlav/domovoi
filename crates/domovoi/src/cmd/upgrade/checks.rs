//! The two checks `install.sh` makes and the upgrade command must make too: the release base
//! URL is safe to download a binary from, and the release is not older than what is installed.

use anyhow::bail;

/// Refuses a release base URL that anyone on the network could change or that hides a login.
///
/// `https` passes. Plain `http` passes only for loopback, or when `allow_http` is set
/// (`DOMOVOI_ALLOW_HTTP=1`). Anything else, a user name or a space is refused, as in
/// `install.sh`.
///
/// # Errors
/// Says what is wrong with the URL and how to allow it where that is possible.
pub fn check_release_base(base: &str, allow_http: bool) -> anyhow::Result<()> {
    if base.contains('@') || base.contains(' ') {
        bail!("DOMOVOI_RELEASE_BASE must not contain a user name or a space");
    }
    if base.starts_with("https://") {
        return Ok(());
    }
    if let Some(rest) = base.strip_prefix("http://") {
        let host = rest.split(['/', ':']).next().unwrap_or("");
        let loopback = host == "127.0.0.1" || host == "localhost" || rest.starts_with("[::1]");
        if loopback || allow_http {
            return Ok(());
        }
        bail!(
            "DOMOVOI_RELEASE_BASE uses plain http, which anyone on the network can change; \
             use https, or set DOMOVOI_ALLOW_HTTP=1 to allow it"
        );
    }
    bail!("DOMOVOI_RELEASE_BASE must start with https://")
}

/// `(major, minor, patch, is_release)` of `0.9.9`, `v0.9.9` or `v0.9.9-rc.1`. A pre-release
/// sorts before its release.
fn parse(version: &str) -> Option<(u64, u64, u64, bool)> {
    let text = version.strip_prefix('v').unwrap_or(version);
    let (numbers, release) = match text.find(['-', '+']) {
        Some(at) if text.as_bytes()[at] == b'-' => (&text[..at], false),
        Some(at) => (&text[..at], true),
        None => (text, true),
    };
    let mut parts = numbers.split('.').map(|p| p.parse::<u64>().ok());
    let major = parts.next()??;
    let minor = parts.next()??;
    let patch = parts.next()??;
    parts.next().is_none().then_some((major, minor, patch, release))
}

/// Refuses to replace `installed` with an older `target` unless `allow_downgrade`. The same
/// version is allowed (a reinstall). A version that cannot be read is refused: the check must
/// not pass by not understanding.
///
/// # Errors
/// Says which versions were compared and the flag that allows it.
pub fn check_not_older(installed: &str, target: &str, allow_downgrade: bool) -> anyhow::Result<()> {
    let (Some(have), Some(want)) = (parse(installed), parse(target)) else {
        bail!("cannot compare the installed version {installed} with {target}");
    };
    if want < have && !allow_downgrade {
        bail!(
            "downgrade: {installed} is installed and {target} is older; pass --allow-downgrade \
             to go back (a hub database may not open on an older release)"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{check_not_older, check_release_base};

    #[test]
    fn https_and_loopback_http_pass_other_http_needs_the_opt_in() {
        assert!(check_release_base("https://github.com/x/y/releases", false).is_ok());
        assert!(check_release_base("http://127.0.0.1:8000", false).is_ok());
        assert!(check_release_base("http://localhost/r", false).is_ok());
        assert!(check_release_base("http://[::1]:80", false).is_ok());
        assert!(check_release_base("http://mirror.example.net/r", false).is_err());
        assert!(check_release_base("http://mirror.example.net/r", true).is_ok());
    }

    #[test]
    fn odd_bases_are_refused() {
        for bad in [
            "ftp://x/r",
            "mirror.example.net",
            "https://user@host/r",
            "https://a b/r",
            "http://127.0.0.1.evil.net/r",
            "",
        ] {
            assert!(check_release_base(bad, true).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn an_older_release_is_refused_unless_allowed() {
        assert!(check_not_older("0.0.10", "v0.0.9", false).is_err());
        assert!(check_not_older("0.0.10", "v0.0.9", true).is_ok());
        assert!(check_not_older("0.0.10", "v0.0.10", false).is_ok());
        assert!(check_not_older("0.0.10", "v0.1.0", false).is_ok());
        assert!(check_not_older("0.0.10", "v0.0.10-rc.1", false).is_err());
        assert!(check_not_older("0.0.10-rc.1", "v0.0.10", false).is_ok());
        assert!(check_not_older("0.0.10", "latest", false).is_err());
    }
}
