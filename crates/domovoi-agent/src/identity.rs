//! The agent's identity: its ids, its ed25519 key and the pinned hub certificate.

use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use domovoi_core::{AgentId, HostId};
use domovoi_proto::{CertFingerprint, PublicKey, Signature};
use ed25519_dalek::{Signer, SigningKey};
use serde::{Deserialize, Serialize};

use crate::error::StateError;
use crate::fs_atomic::{read_optional, write_atomic};

/// The identity file's name under the state directory.
pub const IDENTITY_FILE: &str = "identity.json";
/// Only the agent's user may read the identity file.
pub const IDENTITY_MODE: u32 = 0o600;

/// Who this agent is to the hub, and which hub it trusts.
#[derive(Clone)]
pub struct Identity {
    /// The id the hub assigned at enrollment.
    pub agent_id: AgentId,
    /// The host this agent runs on, as the hub knows it.
    pub host_id: HostId,
    signing_key: SigningKey,
    /// SHA-256 of the hub's certificate, pinned at enrollment.
    pub hub_fingerprint: [u8; 32],
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity")
            .field("agent_id", &self.agent_id)
            .field("host_id", &self.host_id)
            .field("signing_key", &"[hidden]")
            .field("hub_fingerprint", &hex::encode(self.hub_fingerprint))
            .finish()
    }
}

#[derive(Serialize, Deserialize)]
struct IdentityFile {
    agent_id: AgentId,
    host_id: HostId,
    signing_key: String,
    hub_fingerprint: CertFingerprint,
}

/// A fresh ed25519 key for enrollment.
#[must_use]
pub fn generate_key() -> SigningKey {
    SigningKey::from_bytes(&rand::random::<[u8; 32]>())
}

impl Identity {
    /// An identity from its parts.
    #[must_use]
    pub fn new(
        agent_id: AgentId,
        host_id: HostId,
        signing_key: SigningKey,
        hub_fingerprint: [u8; 32],
    ) -> Self {
        Self {
            agent_id,
            host_id,
            signing_key,
            hub_fingerprint,
        }
    }

    /// Where the identity lives under `state_dir`.
    #[must_use]
    pub fn path(state_dir: &Path) -> PathBuf {
        state_dir.join(IDENTITY_FILE)
    }

    /// The public half of the signing key.
    #[must_use]
    pub fn public_key(&self) -> PublicKey {
        PublicKey(self.signing_key.verifying_key().to_bytes())
    }

    /// Signs `message` with the agent's key.
    #[must_use]
    pub fn sign(&self, message: &[u8]) -> Signature {
        Signature(self.signing_key.sign(message).to_bytes())
    }

    /// Loads the identity from `state_dir`, or `None` when the agent is not enrolled.
    pub fn load(state_dir: &Path) -> Result<Option<Self>, StateError> {
        let path = Self::path(state_dir);
        let Some(bytes) = read_optional(&path)? else {
            return Ok(None);
        };
        let file: IdentityFile =
            serde_json::from_slice(&bytes).map_err(|e| StateError::new(&path, e))?;
        let seed = STANDARD
            .decode(file.signing_key.as_bytes())
            .map_err(|e| StateError::new(&path, e))?;
        let seed: [u8; 32] = seed
            .try_into()
            .map_err(|_| StateError::new(&path, "signing key is not 32 bytes"))?;
        Ok(Some(Self {
            agent_id: file.agent_id,
            host_id: file.host_id,
            signing_key: SigningKey::from_bytes(&seed),
            hub_fingerprint: file.hub_fingerprint.0,
        }))
    }

    /// Saves the identity to `state_dir` atomically, readable by the owner only.
    pub fn save(&self, state_dir: &Path) -> Result<(), StateError> {
        let path = Self::path(state_dir);
        let file = IdentityFile {
            agent_id: self.agent_id,
            host_id: self.host_id,
            signing_key: STANDARD.encode(self.signing_key.to_bytes()),
            hub_fingerprint: CertFingerprint(self.hub_fingerprint),
        };
        let bytes = serde_json::to_vec_pretty(&file).map_err(|e| StateError::new(&path, e))?;
        write_atomic(&path, &bytes, IDENTITY_MODE)
    }
}
