//! Password hashing with argon2id (m = 19 MiB, t = 2, p = 1), stored as PHC strings.

use argon2::{Algorithm, Argon2, Params, PasswordHasher, PasswordVerifier, Version};

/// The shortest password accepted.
pub const MIN_PASSWORD_CHARS: usize = 12;

fn hasher() -> Result<Argon2<'static>, String> {
    let params = Params::new(19 * 1024, 2, 1, None).map_err(|e| e.to_string())?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

/// The PHC string for `password` with a fresh random salt.
pub fn hash(password: &str) -> Result<String, String> {
    let salt: [u8; 16] = rand::random();
    let hashed = hasher()?
        .hash_password_with_salt(password.as_bytes(), &salt)
        .map_err(|e| e.to_string())?;
    Ok(hashed.to_string())
}

/// Whether `password` matches the PHC string `stored`. Malformed hashes never match.
#[must_use]
pub fn verify(password: &str, stored: &str) -> bool {
    hasher().is_ok_and(|h| h.verify_password(password.as_bytes(), stored).is_ok())
}

/// A hash to verify against when the user does not exist, so a missing user takes as
/// long as a wrong password.
pub fn dummy_hash() -> &'static str {
    static DUMMY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    DUMMY.get_or_init(|| hash("not a real password, only for timing").unwrap_or_default())
}
