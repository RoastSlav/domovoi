//! A check that cannot see what it watches: how its incident is recognised, and what it says.
//!
//! The incident is about the monitoring, not about the host: the check already knows why it
//! is blind, and a model has nothing to read that the check could not. So the incident says
//! why and how to fix it, in words built here, and no investigation starts by itself.

use crate::host::HostName;
use crate::incident::Incident;
use crate::signal::{CheckKey, EvidenceValue, Signal};

/// The target of the signal that says a check cannot read the journal it watches. The colon
/// keeps it apart from user, unit and pattern names.
pub const BLIND_TARGET: &str = "journal:unreadable";

/// The evidence key under which a blind signal says why the check cannot read.
pub const BLIND_REASON_KEY: &str = "reason";

/// Why a check is blind and how to make it see again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlindNotice {
    /// Why the check cannot read, in the check's own words.
    pub why: String,
    /// What a person does on the host to fix it.
    pub fix: String,
}

impl Incident {
    /// Whether the incident says a check cannot see what it watches.
    #[must_use]
    pub fn is_monitoring_blind(&self) -> bool {
        self.target()
            .is_some_and(|target| target.as_str() == BLIND_TARGET)
    }
}

fn fix(check: &CheckKey, host: &HostName, why: &str) -> String {
    let see = format!(
        "`sudo -u domovoi-agent domovoi check run -c /etc/domovoi/agent.toml --only {check}`"
    );
    let first = if why.contains("systemd-journal") {
        format!(
            "On {host}, run `sudo usermod -aG systemd-journal domovoi-agent` and then \
             `sudo systemctl restart domovoi-agent` (use the account the agent runs as, if it \
             is not domovoi-agent). To check, run {see}."
        )
    } else {
        format!(
            "On {host}, run {see} to see the error, fix what it names, then run \
             `sudo systemctl restart domovoi-agent`."
        )
    };
    format!("{first} The incident resolves at the next successful read.")
}

/// The notice for a blind `incident` on `host`, taking the reason from the newest of
/// `signals` that gives one. `None` for any other incident.
#[must_use]
pub fn blind_notice<'a>(
    incident: &Incident,
    host: &HostName,
    signals: impl IntoIterator<Item = &'a Signal>,
) -> Option<BlindNotice> {
    if !incident.is_monitoring_blind() {
        return None;
    }
    let why = signals
        .into_iter()
        .find_map(|signal| match signal.evidence().get(BLIND_REASON_KEY) {
            Some(EvidenceValue::Text(text)) => Some(text.clone()),
            _ => None,
        })
        .unwrap_or_else(|| "the agent gave no reason".to_owned());
    let fix = fix(incident.check(), host, &why);
    Some(BlindNotice { why, fix })
}
