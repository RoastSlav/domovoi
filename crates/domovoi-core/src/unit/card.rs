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
    let id = identity.id.as_str();
    let mut out = format!("Restart {id} on {host}.\nThis approval binds:\n");
    out.push_str(&format!(
        " - the unit as systemd knows it ({id}); an alias is never bound\n - the unit file {} (sha-256 {})",
        identity.fragment.path,
        short(&identity.fragment.sha256),
    ));
    let shown = identity.drop_ins.iter().take(MAX_PINS_SHOWN);
    let list: Vec<String> = shown
        .map(|p| format!("{} (sha-256 {})", p.path, short(&p.sha256)))
        .collect();
    if !list.is_empty() {
        out.push_str(&format!(" and each drop-in: {}", list.join(", ")));
    }
    let more = identity.drop_ins.len().saturating_sub(MAX_PINS_SHOWN);
    if more > 0 {
        out.push_str(&format!(" and {more} more"));
    }
    let argv = loaded
        .exec_start
        .first()
        .map(|c| c.argv.join(" "))
        .unwrap_or_default();
    out.push_str(&format!(
        "\n - what a restart will run, as loaded now: {argv}, user {}, working directory {}; environment values are hidden\n",
        loaded.user, loaded.working_directory,
    ));
    out.push_str(" - that the unit lives in /etc or /usr/lib and is not masked, linked, transient or generated, and that no drop-in exists on disk that systemd has not loaded\n");
    out.push_str("This approval does not bind:\n");
    out.push_str(" - the contents of EnvironmentFile= files, or of the programs the unit runs\n");
    let follower_text = if followers.is_empty() {
        "none seen".to_owned()
    } else {
        followers.join(", ")
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
