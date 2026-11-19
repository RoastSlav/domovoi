//! The journal position a snapshot read reports as `as_of_seq`.

use super::ApiState;
use super::error::ApiError;

/// The journal head to report as `as_of_seq`.
///
/// A handler reads this **before** it loads the data it describes. The value may then be lower
/// than the true state of the snapshot (an event committed in between is both in the snapshot and
/// replayed on the stream) but never higher, and replays only invalidate, so that is safe.
pub(crate) async fn snapshot_seq(state: &ApiState) -> Result<u64, ApiError> {
    state
        .deps
        .journal
        .head_seq()
        .await
        .map_err(|err| ApiError::internal("reading the journal position", err))
}
