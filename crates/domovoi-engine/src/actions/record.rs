//! The record of one run: what the agent returned, in the shape the audit and journal keep.

use domovoi_core::ports::SkillOutput;
use sha2::{Digest, Sha256};

use super::Shared;
use super::audit::RunRecord;
use crate::triage::cut;

/// The largest output excerpt stored, in bytes.
pub(crate) const EXCERPT_BYTES: usize = 4 * 1024;

pub(crate) fn run_record(shared: &Shared, output: &SkillOutput) -> RunRecord {
    let mut hash = Sha256::new();
    hash.update(output.stdout.as_bytes());
    hash.update([0u8]);
    hash.update(output.stderr.as_bytes());
    let text = if output.stderr.trim().is_empty() {
        output.stdout.clone()
    } else {
        format!("{}\n--- stderr ---\n{}", output.stdout, output.stderr)
    };
    RunRecord {
        exit_code: output.exit_code,
        argv_hash: hex::encode(output.argv_hash),
        output_sha256: hex::encode(hash.finalize()),
        duration_ms: output.duration_ms,
        truncated: output.truncated,
        excerpt: cut(&shared.deps.redactor.redact(&text), EXCERPT_BYTES),
    }
}
