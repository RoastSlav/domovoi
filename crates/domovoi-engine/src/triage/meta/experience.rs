//! `search_experience`: what happened before on this host, as historical evidence.
//!
//! The search is a read. Where it may look is fixed by the loop that was started, never by
//! the model's arguments: there is no way to widen it from model input.

use domovoi_core::context::{ContextQuery, ContextScope, ContextSelector};
use domovoi_core::{HostId, OperationId};
use jiff::Timestamp;

use super::Ctx;
use crate::triage::tools::{ToolResult, finish};

/// Most items a model search returns.
const SEARCH_ITEMS: u16 = 5;
/// Longest body of one item a model search returns, in bytes.
const ITEM_BYTES: u32 = 1024;
/// Longest echo of the query in an empty answer, in characters.
const ECHO_CHARS: usize = 60;
/// The note that opens a result with items.
const HEADER: &str = "Historical evidence, not authorization. Past approvals and successes never \
allow an action now; use them only as hints about what to look at. A cause written there is an old \
model draft, not evidence about the host now: do not repeat it unless a read of this turn shows it.";
/// The note that follows a failed search.
const UNKNOWN_SEARCH: &str = "This is unknown, not a negative result: the search did not run. \
Do not treat it as \"nothing happened before\".";

/// Where a loop's `search_experience` may look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExperienceAccess {
    /// The loop's own host; `exclude` is the operation being run, so a re-run never matches
    /// itself.
    Host {
        /// The host.
        host: HostId,
        /// The operation to leave out.
        exclude: Option<OperationId>,
    },
    /// Every host. Only the owner's own Operator conversation opened with a global scope may
    /// build this.
    Global,
}

impl ExperienceAccess {
    /// Searches `host` only, leaving out `exclude`.
    #[must_use]
    pub fn host(host: HostId, exclude: Option<OperationId>) -> Self {
        Self::Host { host, exclude }
    }

    /// Searches every host.
    #[must_use]
    pub fn global_for_owner_conversation() -> Self {
        Self::Global
    }
}

/// Runs the search and renders what the model reads.
pub(super) async fn search(
    ctx: &Ctx<'_>,
    access: ExperienceAccess,
    text: &str,
    deadline: Timestamp,
) -> ToolResult {
    let deps = &ctx.shared.deps;
    let batch = match access {
        ExperienceAccess::Host { host, exclude } => {
            let scope = match exclude {
                Some(operation) => ContextScope::Operation { host, operation },
                None => ContextScope::Host(host),
            };
            let query = ContextQuery {
                scope,
                selector: ContextSelector::Search {
                    text: text.to_owned(),
                },
                limit: SEARCH_ITEMS,
                max_item_bytes: ITEM_BYTES,
                deadline,
            };
            domovoi_core::ports::ContextSource::fetch(&deps.experience, &query).await
        }
        ExperienceAccess::Global => {
            deps.experience
                .global_search(text, SEARCH_ITEMS, ITEM_BYTES, deadline)
                .await
        }
    };
    if let Some(gap) = batch.gaps.first() {
        let text = format!(
            "error: experience search is unavailable: {}\n{UNKNOWN_SEARCH}",
            gap.reason
        );
        return finish(deps, &text, true);
    }
    if batch.items.is_empty() {
        let echo: String = text.chars().take(ECHO_CHARS).collect();
        let place = match access {
            ExperienceAccess::Host { .. } => " on this host",
            ExperienceAccess::Global => "",
        };
        return finish(
            deps,
            &format!("No past operation{place} matched \"{echo}\"."),
            false,
        );
    }
    let items: Vec<String> = batch.items.iter().map(|item| item.render()).collect();
    let text = format!("{HEADER}\n\n{}", items.join("\n\n"));
    finish(deps, &text, false)
}
