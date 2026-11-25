//! Taking the state lock for an attestation, with 47a's [`StateLock`].

use std::path::Path;

use domovoi_core::helper::UnsignedToken;

use crate::state::StateLock;

/// Opens `state.lock` in `dir`. Any failure is `lock_unavailable`, never `state_corrupt`: a
/// lock that cannot be taken says nothing about the state.
pub(super) fn open(dir: &Path) -> Result<StateLock, UnsignedToken> {
    StateLock::open(dir).map_err(|err| {
        tracing::error!(%err, "the state lock cannot be opened");
        UnsignedToken::LockUnavailable
    })
}
