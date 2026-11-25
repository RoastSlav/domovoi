//! Asking a helper for an attestation and settling the answer.

use domovoi_core::HostId;
use domovoi_core::attest::{IncompleteReason, LatestOutcome, UnverifiedReason};
use domovoi_core::ports::{AttestationReply, CommitError, LinkError, StoreError};
use jiff::SignedDuration;

use super::HelperTrustHub;
use super::verify::{Received, Verdict, verify};

/// How soon after the last request a re-check is refused.
pub const RECHECK_MIN_GAP: SignedDuration = SignedDuration::from_secs(10);

/// How long the hub waits for one answer.
const ASK_TIMEOUT: SignedDuration = SignedDuration::from_secs(15);

/// How many times one request asks again after its acceptance lost a race.
const MAX_ASKS: usize = 2;

/// What one request for a host's attestation came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attempt {
    /// The host has no pinned key, so nothing was asked.
    NotPinned,
    /// A request for this host is already running.
    Busy,
    /// The report verified and is stored.
    Verified,
    /// The reply was refused and nothing about the host's trust changed.
    Rejected {
        /// The wire reason.
        reason: UnverifiedReason,
        /// The internal detail.
        detail: String,
    },
    /// The reply changed the latest outcome and revoked the verified state.
    Revoked {
        /// The new latest outcome.
        outcome: LatestOutcome,
        /// The wire reason.
        reason: UnverifiedReason,
        /// The internal detail.
        detail: String,
    },
    /// The helper or the agent answered with an unsigned token. Unverified; unlocks nothing.
    Unsigned {
        /// The token, sanitised.
        token: String,
    },
    /// No answer: the link timed out or the host is not reachable. Nothing is revoked.
    NoAnswer {
        /// `link_timeout` or `link_unavailable`.
        detail: &'static str,
    },
}

/// A token an agent relayed, kept only when it is a well-formed token.
fn sanitised(token: &str) -> String {
    IncompleteReason::new(token).map_or_else(
        || "malformed_token".to_owned(),
        |token| token.as_str().to_owned(),
    )
}

pub(super) fn store_error(err: CommitError) -> StoreError {
    match err {
        CommitError::Constraint(message) => StoreError::Conflict(message),
        other => StoreError::Backend(other.to_string()),
    }
}

enum Step {
    Done(Attempt),
    Again,
}

impl HelperTrustHub {
    /// Asks `host`'s helper for an attestation and settles the answer: verify, commit, audit.
    /// One request per host runs at a time; a host without a pinned key is not asked.
    ///
    /// # Errors
    /// A store failure.
    pub async fn request(&self, host: HostId) -> Result<Attempt, StoreError> {
        let Some(_slot) = self.begin(host) else {
            return Ok(Attempt::Busy);
        };
        for _ in 0..MAX_ASKS {
            match self.ask_once(host).await? {
                Step::Done(attempt) => return Ok(attempt),
                Step::Again => {}
            }
        }
        Ok(Attempt::Busy)
    }

    async fn ask_once(&self, host: HostId) -> Result<Step, StoreError> {
        let stored = self.stored_with_live(host).await?;
        let Some(pin) = stored.view.pin.clone() else {
            return Ok(Step::Done(Attempt::NotPinned));
        };
        let now = self.clock.now();
        let nonce = self.nonces.issue(host, now);
        self.note_attempt(host, Some(now), None);
        let deadline = now.checked_add(ASK_TIMEOUT).unwrap_or(now);
        let reply = match self.relay.ask(host, nonce, deadline).await {
            Ok(reply) => reply,
            Err(LinkError::Timeout) => {
                return Ok(Step::Done(Attempt::NoAnswer {
                    detail: "link_timeout",
                }));
            }
            Err(_) => {
                return Ok(Step::Done(Attempt::NoAnswer {
                    detail: "link_unavailable",
                }));
            }
        };
        let (public_key, report, signature) = match reply {
            AttestationReply::Unsigned { token } => {
                let token = sanitised(&token);
                self.note_attempt(host, None, Some(token.clone()));
                return Ok(Step::Done(Attempt::Unsigned { token }));
            }
            AttestationReply::Signed {
                public_key,
                report,
                signature,
            } => (public_key, report, signature),
        };
        let seen = self.repo.high_water(host, &pin.fingerprint).await?;
        let received = Received {
            public_key: &public_key,
            report: &report,
            signature: &signature,
        };
        let verdict = verify(
            host,
            Some(&pin),
            &received,
            &self.nonces,
            seen.as_ref(),
            self.clock.now(),
        );
        match verdict {
            Verdict::Accepted(verified) => {
                match self
                    .commit_accepted(host, &pin, verified, seen, &stored)
                    .await
                {
                    Ok(()) => Ok(Step::Done(Attempt::Verified)),
                    Err(CommitError::Conflict { .. }) => Ok(Step::Again),
                    Err(err) => Err(store_error(err)),
                }
            }
            Verdict::Rejected { reason, detail } => {
                Ok(Step::Done(Attempt::Rejected { reason, detail }))
            }
            Verdict::Revoking {
                outcome,
                reason,
                detail,
            } => {
                self.commit_outcome(host, outcome, reason, &detail, &stored)
                    .await
                    .map_err(store_error)?;
                Ok(Step::Done(Attempt::Revoked {
                    outcome,
                    reason,
                    detail,
                }))
            }
        }
    }
}
