//! Protocol versions, release versions, and the exact rule for whether peers may talk.

use std::fmt;

/// A protocol version. The major number is also the envelope's `v` field.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct ProtocolVersion {
    /// Incremented for breaking changes; peers with different majors never talk.
    pub major: u16,
    /// Incremented for additive changes.
    pub minor: u16,
}

/// The version this build speaks.
pub const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion { major: 1, minor: 1 };

/// Whether a hub at `hub` accepts an agent at `agent`: the same major and the same minor.
/// Patch releases do not enter into it.
#[must_use]
pub fn compatible(agent: ProtocolVersion, hub: ProtocolVersion) -> bool {
    agent.major == hub.major && agent.minor == hub.minor
}

impl fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// A release version, as `major.minor.patch`. A release number is not a compatibility
/// dimension: only the protocol version decides whether peers may talk.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct Release {
    /// The breaking release.
    pub major: u64,
    /// The additive release.
    pub minor: u64,
    /// The build.
    pub patch: u64,
}

/// `version` as a [`Release`], ignoring a leading `v` and a `-` or `+` suffix.
/// `None` when it is not `major.minor.patch` of plain numbers, such as `dev` or `1.2`.
pub fn parse_release(version: &str) -> Option<Release> {
    let core = version
        .trim()
        .trim_start_matches('v')
        .split(['-', '+'])
        .next()?;
    let mut parts = core.split('.').map(str::parse::<u64>);
    let release = Release {
        major: parts.next()?.ok()?,
        minor: parts.next()?.ok()?,
        patch: parts.next()?.ok()?,
    };
    parts.next().is_none().then_some(release)
}

/// Whether `tag` is safe wherever a release tag ends up: in a URL path, and on the
/// command line of a script the operator pastes into a root shell. It must parse as a
/// [`Release`] and hold only letters, digits, `.`, `-` and `+`, which covers `v0.9.9` and
/// `0.9.9-rc.2` and rules out every character a shell or a URL gives meaning to (`$`,
/// backticks, quotes, `;`, `|`, `&`, spaces, globs). `parse_release` alone is not enough
/// for this: it stops at the first `-` or `+`, so the junk in `v0.9.9-$(id)` is never
/// looked at and would reach the command.
#[must_use]
pub fn is_release_tag(tag: &str) -> bool {
    parse_release(tag).is_some()
        && tag
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '+')
}

impl fmt::Display for Release {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[cfg(test)]
mod tests {
    use super::{is_release_tag, parse_release};

    #[test]
    fn release_ignores_a_leading_v_and_a_suffix() {
        assert_eq!(parse_release("v0.0.5"), parse_release("0.0.5"));
        assert_eq!(parse_release("0.0.5-rc.2"), parse_release("0.0.5"));
        assert_eq!(parse_release("0.0.5+dirty"), parse_release("0.0.5"));
        assert_eq!(parse_release(" 0.0.5 "), parse_release("0.0.5"));
    }

    #[test]
    fn tags_holding_shell_syntax_are_not_release_tags() {
        assert!(
            is_release_tag("v0.9.9"),
            "the tag a release carries is accepted"
        );
        assert!(is_release_tag("0.9.9"), "the tag without the v is accepted");
        assert!(is_release_tag("0.9.9-rc.2"), "a plain suffix is accepted");
        assert!(
            !is_release_tag("v0.9.9-$(id)"),
            "a command substitution is refused"
        );
        assert!(!is_release_tag("v0.9.9`id`"), "backticks are refused");
        assert!(
            !is_release_tag("v0.9.9' ; rm -rf /"),
            "quotes, a space and a command are refused"
        );
        assert!(
            !is_release_tag("v0.9.9;reboot"),
            "a statement separator is refused"
        );
        assert!(
            !is_release_tag("v0.9.9 *"),
            "a glob and a space are refused"
        );
        assert!(!is_release_tag("v0.9.9\nreboot"), "a newline is refused");
        assert!(!is_release_tag("dev"), "not a release at all");
        assert!(
            !is_release_tag("1.2"),
            "a short version is not a release tag"
        );
    }
}
