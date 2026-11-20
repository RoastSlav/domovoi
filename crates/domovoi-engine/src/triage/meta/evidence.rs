//! `request_evidence`: one read through the evidence source, recorded with what it came to.

use domovoi_core::ReadToolCall;
use domovoi_core::context::{ContextQuery, ContextScope, ContextSelector, ItemStatus};
use jiff::Timestamp;

use super::dispatch::State;
use super::{Ctx, EvidenceRecord};
use crate::context::EvidenceSource;
use crate::triage::tools::{
    EvidenceClass, MAX_RESULT_BYTES, Scope, ToolResult, UNKNOWN_NOTE, cut, finish,
};

/// Longest summary of a recorded read, in characters.
const SUMMARY_CHARS: usize = 280;

/// Longest output of a recorded read, in bytes.
const OUTPUT_BYTES: usize = 16 * 1024;

/// What a read refused for a spent tool-call budget is recorded with.
pub(super) const CALLS_SPENT: &str = "the tool-call budget is spent";

/// What a read refused for a spent evidence budget is recorded with.
const EVIDENCE_SPENT: &str = "the operation's evidence budget is spent";

/// The record of a read that was not run.
pub(super) fn refused(call: ReadToolCall, reason: &str) -> EvidenceRecord {
    EvidenceRecord {
        call,
        class: EvidenceClass::Refused(reason.to_owned()),
        summary: reason.to_owned(),
        output: reason.to_owned(),
    }
}

/// Runs `call` within the loop's evidence budget, records it and returns what the model reads.
///
/// A known answer reads exactly as a read always did; an unknown one reads `error: <text>`
/// followed by [`UNKNOWN_NOTE`]; a refused one reads `error: <text>`.
pub(super) async fn request(
    ctx: &Ctx<'_>,
    state: &mut State<'_>,
    call: ReadToolCall,
    deadline: Timestamp,
    records: &mut Vec<EvidenceRecord>,
) -> ToolResult {
    let deps = &ctx.shared.deps;
    if state.evidence_left == Some(0) {
        records.push(refused(call, EVIDENCE_SPENT));
        return finish(deps, &format!("error: {EVIDENCE_SPENT}; call finish"), true);
    }
    let query = ContextQuery {
        scope: context_scope(ctx.scope),
        selector: ContextSelector::Read { call: call.clone() },
        limit: 1,
        max_item_bytes: u32::try_from(MAX_RESULT_BYTES).unwrap_or(u32::MAX),
        deadline,
    };
    let source = EvidenceSource::new(deps.clone());
    let read = source.read(&query, &call).await;
    let item = read.batch.items.into_iter().next();
    let (result, text) = match (&read.class, item) {
        (EvidenceClass::Observed, Some(item)) if item.status == ItemStatus::Known => {
            // The read happened and is recorded with its real text; the model is told when
            // it is the same text it already has, instead of reading it again.
            let note = state.ledger.check(
                &item.id,
                &item.body,
                u32::from(state.calls_used),
                deps.clock.now(),
            );
            (
                ToolResult {
                    content: note.unwrap_or_else(|| item.body.clone()),
                    is_error: false,
                },
                item.body,
            )
        }
        (class, _) => {
            let reason = class
                .reason()
                .unwrap_or("the read gave no answer")
                .to_owned();
            let unknown = !matches!(class, EvidenceClass::Refused(_));
            // The note is part of the answer, so the bound applies to the reason and the
            // note is kept whole beside it.
            let room = if unknown {
                MAX_RESULT_BYTES.saturating_sub(UNKNOWN_NOTE.len() + 1)
            } else {
                MAX_RESULT_BYTES
            };
            let head = cut(&deps.redactor.redact(&format!("error: {reason}")), room);
            let content = if unknown {
                format!("{head}\n{UNKNOWN_NOTE}")
            } else {
                head
            };
            let result = ToolResult {
                content,
                is_error: true,
            };
            (result, reason)
        }
    };
    if !matches!(read.class, EvidenceClass::Refused(_))
        && let Some(left) = state.evidence_left.as_mut()
    {
        *left = left.saturating_sub(1);
    }
    records.push(EvidenceRecord {
        call,
        class: read.class,
        summary: text.chars().take(SUMMARY_CHARS).collect(),
        output: cut(&text, OUTPUT_BYTES),
    });
    result
}

fn context_scope(scope: Scope) -> ContextScope {
    match scope.incident {
        Some(incident) => ContextScope::Incident {
            host: scope.host,
            incident,
        },
        None => ContextScope::Host(scope.host),
    }
}
