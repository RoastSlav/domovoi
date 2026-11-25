//! Who may ask the runner for what. One layer next to the socket mode and the mount mask.

use domovoi_core::skill_runner::RunnerRequest;

/// The kind of a request, as far as the peer rule is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestKind {
    /// Run a skill.
    Run,
    /// Ask for the version and roots.
    Health,
    /// Ask for the isolation observations.
    Probe,
}

impl RequestKind {
    /// The kind of `request`.
    #[must_use]
    pub fn of(request: &RunnerRequest) -> Self {
        match request {
            RunnerRequest::Run(_) => Self::Run,
            RunnerRequest::Health(_) => Self::Health,
            RunnerRequest::Probe(_) => Self::Probe,
        }
    }
}

/// Whether `peer` may make a `kind` request.
///
/// `Run` is allowed only to the control agent's uid (resolved by name by the caller, `None`
/// when it cannot be) and never to uid 0. `Health` and `Probe` are read-only and also allowed
/// to root, because the installer's self-test drives them as root. An unknown peer is refused.
#[must_use]
pub fn peer_allowed(peer: Option<u32>, kind: RequestKind, agent_uid: Option<u32>) -> bool {
    let Some(peer) = peer else {
        return false;
    };
    let is_agent = agent_uid == Some(peer) && peer != 0;
    match kind {
        RequestKind::Run => is_agent,
        RequestKind::Health | RequestKind::Probe => is_agent || peer == 0,
    }
}
