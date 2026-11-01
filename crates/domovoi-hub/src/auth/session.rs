//! Sessions: a random id in an HttpOnly cookie, only its hash stored, a CSRF token derived
//! from the id, an idle timeout that slides with use and an absolute limit.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jiff::{SignedDuration, Timestamp};
use sha2::{Digest, Sha256};

/// The session cookie's name.
pub const COOKIE: &str = "domovoi_session";

/// A session ends after this long without use.
pub const IDLE: SignedDuration = SignedDuration::from_hours(12);

/// A session ends this long after login, used or not.
pub const ABSOLUTE: SignedDuration = SignedDuration::from_hours(24 * 7);

/// How often use pushes the idle deadline forward.
pub const EXTEND_EVERY: SignedDuration = SignedDuration::from_mins(1);

/// A new session id: 32 random bytes, URL-safe base64.
#[must_use]
pub fn new_id() -> String {
    let bytes: [u8; 32] = rand::random();
    URL_SAFE_NO_PAD.encode(bytes)
}

/// The stored hash of session id `id`.
#[must_use]
pub fn id_hash(id: &str) -> [u8; 32] {
    Sha256::digest(id.as_bytes()).into()
}

/// The CSRF token of session `id`. Only the holder of the HttpOnly cookie can compute it.
#[must_use]
pub fn csrf_token(id: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(b"domovoi-csrf:v1\n");
    hash.update(id.as_bytes());
    hex::encode(hash.finalize())
}

/// The stored hash of a CSRF token.
#[must_use]
pub fn csrf_hash(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

/// The idle deadline for a session created at `created` and used at `now`.
#[must_use]
pub fn deadline(created: Timestamp, now: Timestamp) -> Timestamp {
    let idle = now.checked_add(IDLE).unwrap_or(now);
    let absolute = created.checked_add(ABSOLUTE).unwrap_or(created);
    idle.min(absolute)
}

/// The `Set-Cookie` value for session `id`. `secure` adds `Secure` for HTTPS hubs.
#[must_use]
pub fn set_cookie(id: &str, secure: bool) -> String {
    let secure = if secure { "; Secure" } else { "" };
    format!(
        "{COOKIE}={id}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}{secure}",
        ABSOLUTE.as_secs()
    )
}

/// The `Set-Cookie` value that removes the session cookie.
#[must_use]
pub fn clear_cookie(secure: bool) -> String {
    let secure = if secure { "; Secure" } else { "" };
    format!("{COOKIE}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0{secure}")
}

/// The session id in a `Cookie` header, if any.
#[must_use]
pub fn from_cookie_header(header: &str) -> Option<&str> {
    header.split(';').find_map(|pair| {
        let (name, value) = pair.trim().split_once('=')?;
        (name == COOKIE && !value.is_empty()).then_some(value)
    })
}
