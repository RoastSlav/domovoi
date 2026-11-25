//! The pure part of an attestation: which generation a report carries, what state follows it,
//! and the report values themselves.

use domovoi_core::attest::{
    BootId, CompleteReport, ConfigLines, Hash32, IncompleteReason, IncompleteReport,
    MAX_REPORT_BYTES, Nonce32, RouteLines, render_complete, render_incomplete,
};
use jiff::Timestamp;

use super::state::StateFile;

/// The generation a report carries and the state to commit before signing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The generation the report will carry.
    pub generation: u64,
    /// The state to make durable first; `None` when nothing changed.
    pub write: Option<StateFile>,
}

/// Why no generation can be assigned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the generation counter is exhausted")]
pub struct GenerationExhausted;

/// Decides the generation from the persisted state and the freshly computed hashes.
///
/// The generation increases by exactly one when either hash differs from the persisted one, and
/// never otherwise: not for a new boot id alone, and not for membership of an nft set (which is
/// not an input). A change followed by its revert advances by two. A new boot id is still
/// written, so the persisted state names the boot the helper last attested in.
///
/// # Errors
///
/// [`GenerationExhausted`] when the counter cannot advance.
pub fn plan(
    state: &StateFile,
    config_hash: Hash32,
    route_state_hash: Hash32,
    boot_id: &BootId,
) -> Result<Plan, GenerationExhausted> {
    let changed = state.config_hash != config_hash || state.route_state_hash != route_state_hash;
    if !changed && state.boot_id == *boot_id {
        return Ok(Plan {
            generation: state.generation,
            write: None,
        });
    }
    let generation = if changed {
        state.generation.checked_add(1).ok_or(GenerationExhausted)?
    } else {
        state.generation
    };
    Ok(Plan {
        generation,
        write: Some(StateFile {
            key_fingerprint: state.key_fingerprint,
            generation,
            config_hash,
            route_state_hash,
            boot_id: boot_id.clone(),
        }),
    })
}

/// What a report is built from, besides the effective sets.
#[derive(Debug, Clone)]
pub struct ReportContext {
    /// The 16-character key label.
    pub helper_key_id: String,
    /// The challenge being answered.
    pub nonce: Nonce32,
    /// The kernel's boot id.
    pub boot_id: BootId,
    /// The time, at second precision.
    pub computed_at: Timestamp,
}

/// Truncates `now` to whole seconds, the precision of the report's `computed_at`.
#[must_use]
pub fn at_second(now: Timestamp) -> Timestamp {
    Timestamp::from_second(now.as_second()).unwrap_or(now)
}

/// The complete report for `generation` and its rendered bytes, or `None` when the bytes would
/// exceed the report size cap.
#[must_use]
pub fn complete_report(
    ctx: &ReportContext,
    generation: u64,
    config: ConfigLines,
    routes: RouteLines,
) -> Option<(CompleteReport, Vec<u8>)> {
    let report = CompleteReport {
        helper_key_id: ctx.helper_key_id.clone(),
        nonce: ctx.nonce,
        boot_id: ctx.boot_id.clone(),
        generation,
        config_hash: config.hash(),
        route_state_hash: routes.hash(),
        computed_at: ctx.computed_at,
        config,
        routes,
    };
    let bytes = render_complete(&report);
    (bytes.len() <= MAX_REPORT_BYTES).then_some((report, bytes))
}

/// The minimal incomplete report for `reason`, rendered; `None` when `reason` is not a token a
/// helper signs.
#[must_use]
pub fn incomplete_bytes(ctx: &ReportContext, reason: &str) -> Option<Vec<u8>> {
    let reason = IncompleteReason::new(reason).filter(IncompleteReason::is_known)?;
    Some(render_incomplete(&IncompleteReport {
        helper_key_id: ctx.helper_key_id.clone(),
        nonce: ctx.nonce,
        boot_id: ctx.boot_id.clone(),
        reason,
        computed_at: ctx.computed_at,
    }))
}
