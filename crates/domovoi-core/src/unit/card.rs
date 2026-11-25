//! The approval card text for a unit restart. Constants with `{..}` placeholders filled by
//! [`render`]; nothing else is free text. Environment values are hidden.

use super::identity::UnitIdentityV1;
use super::props::LoadedProps;

/// The most drop-in pins shown on a card; the rest are summarised by count (all are still
/// bound by the hash).
pub const MAX_PINS_SHOWN: usize = 8;

const NOT_BOUND_RACE: &str = "anything a user with root changes between the last check and the moment systemd acts. A small window remains and cannot be closed from here. After the restart Domovoi checks the new process and raises an alert if it is not running what you approved. It does not restart or stop it again.";

/// The stale variant: no approval can be issued.
pub const STALE_CARD: &str = "systemd has changes for this unit on disk that it has not loaded. Run `systemctl daemon-reload` on the host, then ask again. Domovoi never reloads for you.";

/// The line shown when the platform tier is `untested`.
pub const UNTESTED_LINE: &str = "This platform is not release-tested; restarts are on only because you switched them on for this host.";

fn short(hash: &str) -> &str {
    hash.get(..12).unwrap_or(hash)
}

/// What a card shows, as plain text facts: built from the helper's outputs at the hub, or from
/// the identity and loaded properties on the host side. Environment values are never in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardFacts {
    /// The canonical id.
    pub id: String,
    /// The unit file path.
    pub fragment_path: String,
    /// The unit file's SHA-256, lowercase hex.
    pub fragment_sha256: String,
    /// Every drop-in as `(path, sha256)`, in application order.
    pub drop_ins: Vec<(String, String)>,
    /// The `ExecStart` argument vector, joined by spaces.
    pub exec_start: String,
    /// `User`.
    pub user: String,
    /// `WorkingDirectory`.
    pub working_directory: String,
    /// Units that restart or stop with this one.
    pub followers: Vec<String>,
}

impl CardFacts {
    /// The facts of `identity` and `loaded`.
    #[must_use]
    pub fn of(identity: &UnitIdentityV1, loaded: &LoadedProps, followers: &[String]) -> Self {
        Self {
            id: identity.id.to_string(),
            fragment_path: identity.fragment.path.clone(),
            fragment_sha256: identity.fragment.sha256.clone(),
            drop_ins: identity
                .drop_ins
                .iter()
                .map(|p| (p.path.clone(), p.sha256.clone()))
                .collect(),
            exec_start: loaded
                .exec_start
                .first()
                .map(|c| c.argv.join(" "))
                .unwrap_or_default(),
            user: loaded.user.clone(),
            working_directory: loaded.working_directory.clone(),
            followers: followers.to_vec(),
        }
    }
}

/// Renders the approval card for `identity` on `host`.
///
/// `followers` are the units that restart or stop with this one; `untested` adds the
/// untested-platform line.
#[must_use]
pub fn render(
    host: &str,
    identity: &UnitIdentityV1,
    loaded: &LoadedProps,
    followers: &[String],
    untested: bool,
) -> String {
    render_facts(host, &CardFacts::of(identity, loaded, followers), untested)
}

/// Renders the approval card from plain facts.
#[must_use]
pub fn render_facts(host: &str, facts: &CardFacts, untested: bool) -> String {
    let id = &facts.id;
    let mut out = format!("Restart {id} on {host}.\nThis approval binds:\n");
    out.push_str(&format!(
        " - the unit as systemd knows it ({id}); an alias is never bound\n - the unit file {} (sha-256 {})",
        facts.fragment_path,
        short(&facts.fragment_sha256),
    ));
    let list: Vec<String> = facts
        .drop_ins
        .iter()
        .take(MAX_PINS_SHOWN)
        .map(|(path, sha)| format!("{path} (sha-256 {})", short(sha)))
        .collect();
    if !list.is_empty() {
        out.push_str(&format!(" and each drop-in: {}", list.join(", ")));
    }
    let more = facts.drop_ins.len().saturating_sub(MAX_PINS_SHOWN);
    if more > 0 {
        out.push_str(&format!(" and {more} more"));
    }
    out.push_str(&format!(
        "\n - what a restart will run, as loaded now: {}, user {}, working directory {}; environment values are hidden\n",
        facts.exec_start, facts.user, facts.working_directory,
    ));
    out.push_str(" - that the unit lives in /etc or /usr/lib and is not masked, linked, transient or generated, and that no drop-in exists on disk that systemd has not loaded\n");
    out.push_str("This approval does not bind:\n");
    out.push_str(" - the contents of EnvironmentFile= files, or of the programs the unit runs\n");
    let follower_text = if facts.followers.is_empty() {
        "none seen".to_owned()
    } else {
        facts.followers.join(", ")
    };
    out.push_str(&format!(
        " - other units that restart or stop with this one ({follower_text})\n - {NOT_BOUND_RACE}\n"
    ));
    out.push_str("A restart can fail and leave the service stopped (for example when systemd's start limit is hit). Restarting a stopped service starts it.");
    if untested {
        out.push('\n');
        out.push_str(UNTESTED_LINE);
    }
    out
}
