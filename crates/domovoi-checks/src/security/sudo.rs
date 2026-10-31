//! `security.sudo_failure`: failed sudo authentication.
//!
//! - **Source:** journal messages with the `sudo` syslog identifier since the last cursor,
//!   matching `authentication failure` (from PAM) and `incorrect password attempts`.
//! - **Target:** the user. **Severity:** warning. **Class:** immediate.
//! - **Evidence:** `count` (matching lines in this run), `example`.

use std::collections::BTreeMap;

use async_trait::async_trait;
use domovoi_core::{CheckKey, Severity, SignalClass, ValidationError};
use jiff::SignedDuration;

use super::journal_since_cursor;
use crate::commands::{JournalScope, SyslogIdentifier};
use crate::framework::{
    Check, CheckContext, CheckError, CheckReport, Observation, int, key, target,
};

/// Longest example line kept, in characters.
pub const MAX_EXAMPLE_CHARS: usize = 300;

fn field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    line.split(|c: char| c.is_whitespace() || c == ';')
        .filter_map(|word| word.strip_prefix(name))
        .filter_map(|rest| rest.strip_prefix('='))
        .rfind(|value| !value.is_empty())
}

/// The user named in a sudo failure line, or `None` when the line is not one.
#[must_use]
pub fn parse_sudo_failure(line: &str) -> Option<String> {
    let line = line.trim();
    let line = line.strip_prefix("sudo:").map_or(line, str::trim_start);
    if line.contains("authentication failure") {
        let user = field(line, "user").or_else(|| field(line, "ruser"))?;
        return Some(user.to_owned());
    }
    if line.contains("incorrect password attempt") {
        let (user, _) = line.split_once(" : ")?;
        let user = user.trim();
        return (!user.is_empty() && !user.contains(char::is_whitespace)).then(|| user.to_owned());
    }
    None
}

/// The `security.sudo_failure` check.
pub struct SudoFailures {
    key: CheckKey,
    identifiers: Vec<SyslogIdentifier>,
}

impl SudoFailures {
    /// The check.
    pub fn new() -> Result<Self, ValidationError> {
        let sudo = SyslogIdentifier::parse("sudo")
            .map_err(|e| ValidationError::new("syslog identifier", e.to_string()))?;
        Ok(Self {
            key: key("security.sudo_failure")?,
            identifiers: vec![sudo],
        })
    }
}

#[async_trait]
impl Check for SudoFailures {
    fn key(&self) -> CheckKey {
        self.key.clone()
    }

    fn default_cadence(&self) -> SignedDuration {
        SignedDuration::from_mins(1)
    }

    fn class(&self) -> SignalClass {
        SignalClass::Immediate
    }

    async fn run(&self, ctx: &mut CheckContext<'_>) -> Result<CheckReport, CheckError> {
        let lines = journal_since_cursor(ctx, JournalScope::Identifiers(&self.identifiers)).await?;
        let mut by_user: BTreeMap<String, (u64, String)> = BTreeMap::new();
        for line in &lines {
            if let Some(user) = parse_sudo_failure(line) {
                let entry = by_user
                    .entry(user)
                    .or_insert_with(|| (0, line.chars().take(MAX_EXAMPLE_CHARS).collect()));
                entry.0 += 1;
            }
        }
        let mut report = CheckReport::default();
        for (user, (count, example)) in by_user {
            report.observations.push(
                Observation::firing(
                    target(&user),
                    Severity::Warning,
                    format!("failed sudo authentication for {user}"),
                )
                .with("count", int(count))
                .with_text("example", example),
            );
        }
        Ok(report)
    }
}
