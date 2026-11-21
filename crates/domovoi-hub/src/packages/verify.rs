//! Checking a detached signature file against the hub's owner keys and the archive's digest.
//!
//! The hub's key list is a convenience that refuses packages no agent would accept; the agent
//! stays the only authority. Nothing here opens the archive.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use domovoi_core::package::{
    ArchiveDigest, MAX_SIGNATURE_FILE_BYTES, SIGNATURE_ALGORITHM, SIGNATURE_FORMAT, SignatureFile,
    signing_message,
};
use ed25519_dalek::Signature;

use super::error::ImportError;
use super::keys::PackageKeys;

/// Reads the signature file's shape: size, UTF-8, TOML with no unknown field, format and
/// algorithm. Returns the parsed file and its text. It does not check the key or the
/// signature.
///
/// # Errors
/// [`ImportError::SignatureFileInvalid`] naming what is wrong.
pub fn parse_signature_file(bytes: &[u8]) -> Result<(SignatureFile, String), ImportError> {
    let invalid = |reason: String| ImportError::SignatureFileInvalid(reason);
    if bytes.len() > MAX_SIGNATURE_FILE_BYTES {
        return Err(invalid(format!(
            "it is larger than {MAX_SIGNATURE_FILE_BYTES} bytes"
        )));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid("it is not UTF-8 text".into()))?;
    let file: SignatureFile =
        toml::from_str(text).map_err(|err| invalid(format!("it does not parse: {err}")))?;
    if file.format != SIGNATURE_FORMAT {
        return Err(invalid(format!("format {} is not supported", file.format)));
    }
    if file.algorithm != SIGNATURE_ALGORITHM {
        return Err(invalid(format!(
            "algorithm {:?} is not supported",
            file.algorithm
        )));
    }
    Ok((file, text.to_owned()))
}

/// Checks that `file` was signed by a trusted owner key and covers the archive whose digest
/// is `streamed`, and returns that key's label.
///
/// A revoked key is refused before anything else, even when it is also listed. The
/// signature is checked with `verify_strict` over the exact bytes of `signing_message`
/// before the digest is compared, so a mismatch means the signed digest is not the archive's.
///
/// # Errors
/// [`ImportError::RevokedKey`], [`ImportError::UnknownKey`], [`ImportError::BadSignature`]
/// or [`ImportError::DigestMismatch`], in that order.
pub fn verify_signature(
    keys: &PackageKeys,
    file: &SignatureFile,
    streamed: &ArchiveDigest,
) -> Result<Option<String>, ImportError> {
    if keys.is_revoked(&file.key_id) {
        return Err(ImportError::RevokedKey(file.key_id.clone()));
    }
    let Some(owner) = keys.trusted(&file.key_id) else {
        return Err(ImportError::UnknownKey(file.key_id.clone()));
    };
    let raw = STANDARD
        .decode(file.signature.as_bytes())
        .map_err(|_| ImportError::BadSignature)?;
    let signature = Signature::from_slice(&raw).map_err(|_| ImportError::BadSignature)?;
    let message = signing_message(
        &file.package,
        &file.version,
        &file.archive_sha256,
        &file.key_id,
    );
    owner
        .key
        .verify_strict(&message, &signature)
        .map_err(|_| ImportError::BadSignature)?;
    if file.archive_sha256 != *streamed {
        return Err(ImportError::DigestMismatch);
    }
    Ok(owner.label.clone())
}
