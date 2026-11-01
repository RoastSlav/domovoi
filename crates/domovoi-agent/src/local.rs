//! Single-box mode: the agent's identity when it runs inside the hub process.

use std::future::Future;
use std::path::Path;

use domovoi_core::{AgentId, HostId};

use crate::error::AgentError;
use crate::identity::{Identity, generate_key};

/// What the hub answered when the local agent registered its key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Registered {
    /// The agent's id.
    pub agent_id: AgentId,
    /// The hub's own host.
    pub host_id: HostId,
    /// SHA-256 of the hub certificate.
    pub hub_fingerprint: [u8; 32],
}

/// Loads the local agent's identity from `state_dir`, generating a key on first start, and
/// registers its public key with the hub through `register` (local trust: the hub is the
/// same process). Saves the identity again when the hub's answer changed it, for example
/// after the hub's database was reset.
pub async fn local_identity<F, Fut>(state_dir: &Path, register: F) -> Result<Identity, AgentError>
where
    F: FnOnce([u8; 32]) -> Fut,
    Fut: Future<Output = Result<Registered, String>>,
{
    let existing = Identity::load(state_dir)?;
    let key = existing
        .as_ref()
        .map_or_else(generate_key, Identity::signing_key);
    let registered = register(key.verifying_key().to_bytes())
        .await
        .map_err(AgentError::Local)?;
    let identity = Identity::new(
        registered.agent_id,
        registered.host_id,
        key,
        registered.hub_fingerprint,
    );
    let unchanged = existing.as_ref().is_some_and(|old| {
        old.agent_id == identity.agent_id
            && old.host_id == identity.host_id
            && old.hub_fingerprint == identity.hub_fingerprint
    });
    if !unchanged {
        std::fs::create_dir_all(state_dir)
            .map_err(|e| AgentError::State(crate::error::StateError::new(state_dir, e)))?;
        identity.save(state_dir)?;
    }
    Ok(identity)
}

#[cfg(test)]
mod tests {
    use domovoi_core::{AgentId, HostId};

    use super::{Registered, local_identity};

    #[tokio::test]
    async fn first_start_generates_and_saves_then_reuses_the_key() {
        let dir = tempfile::tempdir().unwrap();
        let answer = Registered {
            agent_id: AgentId::new(),
            host_id: HostId::new(),
            hub_fingerprint: [7; 32],
        };
        let first = local_identity(dir.path(), |_| async move { Ok(answer) })
            .await
            .unwrap();
        let mut seen = None;
        let second = local_identity(dir.path(), |key| {
            seen = Some(key);
            async move { Ok(answer) }
        })
        .await
        .unwrap();
        assert_eq!(seen, Some(first.public_key().0), "the saved key is reused");
        assert_eq!(second.agent_id, answer.agent_id);
        assert!(
            local_identity(dir.path(), |_| async { Err("hub said no".to_owned()) })
                .await
                .is_err()
        );
    }
}
