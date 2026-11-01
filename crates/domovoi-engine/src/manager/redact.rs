//! Redacting inbound signal text before anything is stored (INV-5).
//!
//! Every source (agents, webhooks, probes) reaches incidents through the manager's
//! ingest command, so this is the one place webhook payload text is cleaned before it
//! lands in signals, incident titles, cards and API responses.

use domovoi_core::ports::Redactor;
use domovoi_core::{Evidence, EvidenceValue, Signal};

use super::command::Command;

/// `command` with the summary and text evidence of every ingested signal redacted. Other
/// commands pass through unchanged.
pub(super) fn redact_command(redactor: &dyn Redactor, command: Command) -> Command {
    match command {
        Command::Ingest(signals) => Command::Ingest(
            signals
                .into_iter()
                .map(|signal| redact_signal(redactor, signal))
                .collect(),
        ),
        other => other,
    }
}

/// `signal` with its summary and text evidence values redacted. Numbers and flags are
/// kept as they are.
pub(super) fn redact_signal(redactor: &dyn Redactor, signal: Signal) -> Signal {
    let summary = redactor.redact(signal.summary());
    let mut evidence = Evidence::new();
    for (key, value) in signal.evidence().iter() {
        let value = match value {
            EvidenceValue::Text(text) => EvidenceValue::Text(redactor.redact(text)),
            other => other.clone(),
        };
        evidence.insert(key.clone(), value);
    }
    signal.with_summary(summary).with_evidence(evidence)
}
