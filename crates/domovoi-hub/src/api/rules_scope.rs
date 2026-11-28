//! How far a known condition for a security check may reach, and the password it asks for.
//!
//! A rule silences a check: nothing it matches opens an incident or sends a message. For a
//! security check (`security.*`) and for `kernel.errors` the harm of a too-wide rule is a
//! fleet that stops noticing logins, listeners or kernel faults, so such a rule must name a
//! host and a target pattern, or end within [`MAX_WIDE_DAYS`] days. Creating one, or editing
//! what it covers, asks for the password again, as changing settings and policy does.

use std::net::IpAddr;

use domovoi_core::{AckRule, CheckKey};
use jiff::{SignedDuration, Timestamp};

use super::ApiState;
use super::error::ApiError;
use crate::auth::{AuthError, Current};

/// The longest a rule may last when it covers every host or every target of a guarded check.
pub(super) const MAX_WIDE_DAYS: i64 = 30;

/// [`MAX_WIDE_DAYS`] in minutes, for `expires_in_minutes`.
const MAX_WIDE_MINUTES: i64 = MAX_WIDE_DAYS * 24 * 60;

/// Whether `check` is one whose rules are held to this scope: the security checks and
/// `kernel.errors`.
#[must_use]
pub(super) fn is_guarded(check: &CheckKey) -> bool {
    let name = check.as_str();
    name.starts_with("security.") || name == "kernel.errors"
}

/// Whether the rule matches every target: no pattern, or a bare `*`.
#[must_use]
pub(super) fn covers_every_target(rule: &AckRule) -> bool {
    rule.target_pattern
        .as_ref()
        .is_none_or(|pattern| pattern.to_string() == "*")
}

/// Refuses a rule that is too wide for its check, with a sentence that says what to add.
/// `**` is refused for every check: it is not a pattern, `*` at both ends is.
pub(super) fn check_scope(rule: &AckRule, now: Timestamp) -> Result<(), ApiError> {
    if rule
        .target_pattern
        .as_ref()
        .is_some_and(|pattern| pattern.to_string() == "**")
    {
        return Err(ApiError::BadRequest(
            "`**` is not a target pattern. Use `*` before or after a word (`web*`, `*:22/tcp`), \
             or leave the target out to match every target"
                .to_owned(),
        ));
    }
    if !is_guarded(&rule.check) {
        return Ok(());
    }
    let every_host = rule.host.is_none();
    let every_target = covers_every_target(rule);
    if !every_host && !every_target {
        return Ok(());
    }
    let reach = match (every_host, every_target) {
        (true, true) => "every host and every target",
        (true, false) => "every host",
        _ => "every target",
    };
    let latest = now
        .checked_add(SignedDuration::from_mins(MAX_WIDE_MINUTES))
        .unwrap_or(Timestamp::MAX);
    match rule.expires_at {
        Some(at) if at <= latest => Ok(()),
        Some(_) => Err(ApiError::BadRequest(format!(
            "a rule for {} that covers {reach} may last at most {MAX_WIDE_DAYS} days \
             (expires_in_minutes up to {MAX_WIDE_MINUTES}). Shorten the expiry, or name a host \
             and a target pattern to keep it longer",
            rule.check
        ))),
        None => Err(ApiError::BadRequest(format!(
            "a rule for {} would cover {reach} for ever. Name a host and a target pattern, or \
             give it an expiry of at most {MAX_WIDE_DAYS} days (expires_in_minutes up to \
             {MAX_WIDE_MINUTES})",
            rule.check
        ))),
    }
}

/// Asks for the admin's password again. It shares the login limit for `ip`, so it cannot be
/// used to guess faster.
pub(super) async fn require_password(
    state: &ApiState,
    current: &Current,
    ip: IpAddr,
    password: Option<&str>,
) -> Result<(), ApiError> {
    let Some(password) = password else {
        return Err(ApiError::Forbidden(
            "enter your password to make this change".to_owned(),
        ));
    };
    match state
        .auth
        .confirm_password(ip, current.user.id, password)
        .await
    {
        Ok(()) => Ok(()),
        Err(AuthError::LockedOut { retry_after_secs }) => Err(ApiError::TooMany(retry_after_secs)),
        Err(_) => Err(ApiError::Forbidden("wrong password".to_owned())),
    }
}
