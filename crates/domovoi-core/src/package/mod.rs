//! Signed capability packages: the names, digests, signature file and manifest shapes shared
//! by the agent that installs a package and the hub that offers one.
//!
//! Everything here is pure data and grammar. Verifying a signature, reading an archive and
//! writing files are the agent's job; this module only fixes what the bytes mean.

mod digest;
mod failure;
mod manifest;
mod names;
mod output;
mod signature;

pub use digest::ArchiveDigest;
pub use failure::PackageFailureCode;
pub use manifest::{
    MAX_PACKAGE_MANIFEST_BYTES, MAX_SKILLS_PER_PACKAGE, PackageCompat, PackageManifest,
};
pub use names::{KeyId, PackageName, PackageVersion};
pub use output::{KeyState, PackageListEntry, PackageStatus, PrivilegeBoundary, keys};
pub use signature::{
    MAX_INSTALLED_PACKAGES, MAX_SIGNATURE_FILE_BYTES, SIGNATURE_ALGORITHM, SIGNATURE_FORMAT,
    SignatureFile, signing_message,
};

#[cfg(test)]
mod tests;
