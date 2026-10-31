//! Web UI accounts and login sessions.

use jiff::Timestamp;

use crate::id::UserId;

/// A web UI account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    /// Stable identifier.
    pub id: UserId,
    /// Unique login name.
    pub username: String,
    /// The argon2id hash in PHC string format. Never the password itself.
    pub password_hash: String,
    /// When the account was created.
    pub created_at: Timestamp,
}

/// A logged-in web session. Only hashes of the cookie and CSRF tokens are stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// SHA-256 of the session cookie value.
    pub token_hash: [u8; 32],
    /// The account it belongs to.
    pub user: UserId,
    /// SHA-256 of the synchronizer CSRF token.
    pub csrf_hash: [u8; 32],
    /// When the session started.
    pub created_at: Timestamp,
    /// When the session stops being valid.
    pub expires_at: Timestamp,
}
