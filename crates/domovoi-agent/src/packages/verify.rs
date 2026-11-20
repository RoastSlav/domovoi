//! Checks on the detached signature file and the package manifest: shape, identity, trust,
//! signature, digest and compatibility. Nothing here touches the archive's entries.

use std::io::Read;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use domovoi_core::package::{
    ArchiveDigest, MAX_PACKAGE_MANIFEST_BYTES, MAX_SIGNATURE_FILE_BYTES, PackageFailureCode,
    PackageManifest, PackageName, PackageVersion, SIGNATURE_ALGORITHM, SIGNATURE_FORMAT,
    SignatureFile, signing_message,
};
use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest as _, Sha256};

use super::error::{PackageError, quote};

/// The only `skill_contract` value this agent supports.
pub const SUPPORTED_SKILL_CONTRACT: u32 = 1;
/// The only `package.toml` format this agent reads.
pub const MANIFEST_FORMAT: u32 = 1;
/// Longest `description` in `package.toml`, in characters.
const DESCRIPTION_LIMIT: usize = 200;

fn invalid(detail: impl Into<String>) -> PackageError {
    PackageError::new(PackageFailureCode::SignatureFileInvalid, detail)
}

/// Parses `<name>-<version>.sig.toml`: at most 16 KiB, valid UTF-8 TOML with no unknown
/// field, `format = 1`, `algorithm = "ed25519"` and a signature of exactly 64 bytes.
///
/// # Errors
/// `signature_file_invalid`.
pub fn parse_signature_file(bytes: &[u8]) -> Result<SignatureFile, PackageError> {
    if bytes.len() > MAX_SIGNATURE_FILE_BYTES {
        return Err(invalid("the signature file is too large"));
    }
    let text =
        std::str::from_utf8(bytes).map_err(|_| invalid("the signature file is not UTF-8"))?;
    let file: SignatureFile = toml::from_str(text)
        .map_err(|error| invalid(format!("not a signature file: {}", quote(error.message()))))?;
    if file.format != SIGNATURE_FORMAT {
        return Err(invalid(format!("unsupported format {}", file.format)));
    }
    if file.algorithm != SIGNATURE_ALGORITHM {
        return Err(invalid(format!(
            "unsupported algorithm {}",
            quote(&file.algorithm)
        )));
    }
    signature_bytes(&file)?;
    Ok(file)
}

/// The 64 signature bytes of `file`.
fn signature_bytes(file: &SignatureFile) -> Result<[u8; 64], PackageError> {
    let raw = STANDARD
        .decode(file.signature.as_bytes())
        .map_err(|_| invalid("the signature is not valid base64"))?;
    raw.try_into()
        .map_err(|_| invalid("the signature is not 64 bytes"))
}

/// The signature file must be about the package and version the request names.
///
/// # Errors
/// `identity_mismatch`.
pub fn same_identity(
    file: &SignatureFile,
    package: &PackageName,
    version: &PackageVersion,
) -> Result<(), PackageError> {
    if &file.package != package || &file.version != version {
        return Err(PackageError::new(
            PackageFailureCode::IdentityMismatch,
            format!(
                "the signature is for {} {}, not {package} {version}",
                file.package, file.version
            ),
        ));
    }
    Ok(())
}

/// Verifies `file.signature` over exactly [`signing_message`] with `verify_strict`.
///
/// # Errors
/// `bad_signature`, or `signature_file_invalid` when the signature cannot be decoded.
pub fn signature(key: &VerifyingKey, file: &SignatureFile) -> Result<(), PackageError> {
    let bytes = signature_bytes(file)?;
    let message = signing_message(
        &file.package,
        &file.version,
        &file.archive_sha256,
        &file.key_id,
    );
    key.verify_strict(&message, &Signature::from_bytes(&bytes))
        .map_err(|_| {
            PackageError::new(
                PackageFailureCode::BadSignature,
                format!("the signature of key {} does not verify", file.key_id),
            )
        })
}

/// SHA-256 of everything `reader` yields, read in 64 KiB steps.
///
/// # Errors
/// `storage` when the reader fails.
pub fn sha256_stream(mut reader: impl Read) -> Result<ArchiveDigest, PackageError> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| PackageError::io("reading the archive", &error))?;
        let Some(chunk) = buffer.get(..read).filter(|chunk| !chunk.is_empty()) else {
            break;
        };
        hasher.update(chunk);
    }
    Ok(ArchiveDigest::from_bytes(hasher.finalize().into()))
}

/// The archive's digest must be the one the signature covers.
///
/// # Errors
/// `digest_mismatch`.
pub fn check_digest(actual: &ArchiveDigest, file: &SignatureFile) -> Result<(), PackageError> {
    if actual != &file.archive_sha256 {
        return Err(PackageError::new(
            PackageFailureCode::DigestMismatch,
            "the archive does not match the signed digest",
        ));
    }
    Ok(())
}

/// Parses `package.toml` strictly (at most 64 KiB, no unknown field, `format = 1`).
///
/// # Errors
/// `manifest_invalid`.
pub fn parse_manifest(bytes: &[u8]) -> Result<PackageManifest, PackageError> {
    let bad = |detail: String| PackageError::new(PackageFailureCode::ManifestInvalid, detail);
    if bytes.len() > MAX_PACKAGE_MANIFEST_BYTES {
        return Err(bad("package.toml is too large".to_owned()));
    }
    let text =
        std::str::from_utf8(bytes).map_err(|_| bad("package.toml is not UTF-8".to_owned()))?;
    let manifest: PackageManifest = toml::from_str(text).map_err(|error| {
        bad(format!(
            "package.toml is invalid: {}",
            quote(error.message())
        ))
    })?;
    if manifest.format != MANIFEST_FORMAT {
        return Err(bad(format!(
            "unsupported package.toml format {}",
            manifest.format
        )));
    }
    if manifest.description.as_deref().is_some_and(|text| {
        text.chars().count() > DESCRIPTION_LIMIT || text.chars().any(char::is_control)
    }) {
        return Err(bad(
            "description is too long or has control characters".to_owned()
        ));
    }
    if manifest.compat.protocols.is_empty() {
        return Err(bad("compat.protocols must not be empty".to_owned()));
    }
    Ok(manifest)
}

/// The manifest inside the archive must name the signed package and version.
///
/// # Errors
/// `identity_mismatch`.
pub fn manifest_identity(
    manifest: &PackageManifest,
    package: &PackageName,
    version: &PackageVersion,
) -> Result<(), PackageError> {
    if &manifest.name != package || &manifest.version != version {
        return Err(PackageError::new(
            PackageFailureCode::IdentityMismatch,
            format!(
                "package.toml is for {} {}, not {package} {version}",
                manifest.name, manifest.version
            ),
        ));
    }
    Ok(())
}

/// Whether this agent can use the package: its protocol is listed, its platform is listed
/// (or none is), and its skill contract is the one supported.
///
/// # Errors
/// `incompatible`.
pub fn compat(
    manifest: &PackageManifest,
    protocol: &str,
    platform: &str,
) -> Result<(), PackageError> {
    let no = |detail: String| PackageError::new(PackageFailureCode::Incompatible, detail);
    let compat = &manifest.compat;
    if !compat.protocols.iter().any(|listed| listed == protocol) {
        return Err(no(format!(
            "the package does not support protocol {protocol}"
        )));
    }
    if !compat.platforms.is_empty() && !compat.platforms.iter().any(|listed| listed == platform) {
        return Err(no(format!("the package does not support {platform}")));
    }
    if compat.skill_contract != SUPPORTED_SKILL_CONTRACT {
        return Err(no(format!(
            "unsupported skill_contract {}",
            compat.skill_contract
        )));
    }
    Ok(())
}
