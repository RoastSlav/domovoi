//! What happens to actions the hub session that just stopped left unfinished (ADR 0014):
//! an answer the agent produced went to the session that is gone, so an action still
//! waiting for one is failed with a reason that says the host must be checked. Approved
//! actions are not touched here: the executor starts them.

use domovoi_core::ActionChange;
use domovoi_core::ports::{ActionFilter, Page, StoreError};

use super::Shared;
use super::audit;

/// Why an action that was running when the hub stopped is failed. Nothing is known
/// about the run: the agent's answer went to the session that is gone.
pub(crate) const RESTART_REASON: &str =
    "the hub restarted while this ran; check the host before running it again";

/// Fails every action the store says was running and returns how many. The store picks
/// them out, so this reads only leftovers, and the guarded write keeps a decision that
/// came in the meantime from being overwritten.
pub(crate) async fn fail_abandoned(shared: &Shared) -> usize {
    let filter = ActionFilter {
        status: Some("dispatched".to_owned()),
        ..ActionFilter::default()
    };
    let mut failed = 0;
    let mut page = Page::first(Page::MAX_LIMIT);
    loop {
        let got = match shared.deps.actions.list(&filter, &page).await {
            Ok(got) => got,
            Err(err) => {
                tracing::warn!(%err, "could not look for actions left running");
                return failed;
            }
        };
        for mut action in got.items {
            let now = shared.deps.clock.now();
            let ActionChange { from, .. } =
                match action.complete(Err(RESTART_REASON.to_owned()), now) {
                    Ok(change) => change,
                    Err(err) => {
                        tracing::debug!(
                            action = %action.id(),
                            %err,
                            "an action left running already has an outcome",
                        );
                        continue;
                    }
                };
            match shared
                .record_from(
                    &action,
                    from.name(),
                    audit::finished(&action, None, Some(RESTART_REASON), now),
                )
                .await
            {
                Ok(()) => failed += 1,
                Err(StoreError::Backend(what)) => tracing::warn!(
                    %what,
                    action = %action.id(),
                    "could not fail an action left running",
                ),
                Err(err) => tracing::debug!(
                    %err,
                    action = %action.id(),
                    "someone decided this action meanwhile; the restart leaves it as they left it",
                ),
            }
        }
        match got.next_cursor {
            Some(cursor) => page.cursor = Some(cursor),
            None => return failed,
        }
    }
}
