//! Single-use enrollment tokens.
//!
//! A token is 32 random bytes shown once, inside the enroll command. Only its SHA-256 is
//! stored, so a copy of the database cannot enroll an agent.

use std::collections::BTreeMap;
use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use domovoi_core::ports::{EnrollmentToken, HostRepo, StoreError};
use domovoi_core::{Host, HostId, HostKind, HostName};
use domovoi_proto::EnrollToken;
use jiff::{SignedDuration, Timestamp};
use sha2::{Digest, Sha256};
use url::Url;

/// How long a new token stays valid.
pub const TOKEN_TTL: SignedDuration = SignedDuration::from_mins(15);

/// What an operator needs to enroll one agent.
pub struct EnrollmentInvite {
    /// The host the agent will speak for.
    pub host: HostId,
    /// The token itself. Shown once; the hub keeps only its hash.
    pub token: EnrollToken,
    /// When the token stops working.
    pub expires_at: Timestamp,
    /// The hub certificate's fingerprint, colon-separated hex.
    pub fingerprint: String,
    /// The exact command to run on the host.
    pub command: String,
}

impl fmt::Debug for EnrollmentInvite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EnrollmentInvite")
            .field("host", &self.host)
            .field("token", &self.token)
            .field("expires_at", &self.expires_at)
            .field("fingerprint", &self.fingerprint)
            .finish_non_exhaustive()
    }
}

/// SHA-256 of a token as the agent sent it.
#[must_use]
pub fn token_hash(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

/// Creates a token for the host called `name`, registering it as a remote host when it
/// does not exist yet.
pub(crate) async fn create(
    hosts: &dyn HostRepo,
    name: &HostName,
    ttl: SignedDuration,
    now: Timestamp,
    hub_url: &Url,
    fingerprint: &str,
) -> Result<EnrollmentInvite, StoreError> {
    let host = match hosts.get_host_by_name(name).await? {
        Some(host) => host,
        None => {
            let host = Host {
                id: HostId::new(),
                name: name.clone(),
                kind: HostKind::Remote,
                labels: BTreeMap::new(),
                created_at: now,
            };
            hosts.upsert_host(&host).await?;
            host
        }
    };
    let bytes: [u8; 32] = rand::random();
    let token = EnrollToken::new(URL_SAFE_NO_PAD.encode(bytes));
    let expires_at = now.saturating_add(ttl).unwrap_or(now);
    hosts
        .create_enrollment_token(&EnrollmentToken {
            host: host.id,
            token_hash: token_hash(token.expose()),
            expires_at,
        })
        .await?;
    let command = format!(
        "domovoi agent enroll --hub {hub_url} --token {} --fingerprint {fingerprint}",
        token.expose()
    );
    Ok(EnrollmentInvite {
        host: host.id,
        token,
        expires_at,
        fingerprint: fingerprint.to_owned(),
        command,
    })
}
