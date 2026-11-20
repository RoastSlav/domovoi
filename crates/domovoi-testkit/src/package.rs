//! Builds signed `<name>-<version>.tar.zst` packages and their detached `.sig.toml` for tests.
//!
//! Hostile archives are first-class: [`PackageFixture::raw_entry`] writes the tar name field
//! byte for byte, because `tar::Header::set_path` refuses `..` and absolute paths.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use domovoi_core::package::{
    ArchiveDigest, KeyId, PackageName, PackageVersion, SIGNATURE_ALGORITHM, SIGNATURE_FORMAT,
    signing_message,
};
use ed25519_dalek::{Signer as _, SigningKey};
use ruzstd::encoding::{CompressionLevel, compress_to_vec};
use sha2::{Digest as _, Sha256};
use tar::{Builder, EntryType, Header};

use crate::build::BuildError;

/// A package ready to hand to the agent: archive bytes, the signature file and the key.
#[derive(Debug, Clone)]
pub struct BuiltPackage {
    /// The `.tar.zst` bytes (`package_artifact`).
    pub archive: Vec<u8>,
    /// The `.sig.toml` text (`package_manifest`).
    pub sig_toml: String,
    /// The key that signed it.
    pub signing_key: SigningKey,
    /// That key's id.
    pub key_id: KeyId,
    /// Standard base64 of the 32 public-key bytes, as written in `agent.toml`.
    pub public_key_b64: String,
    /// SHA-256 of `archive`.
    pub archive_sha256: ArchiveDigest,
}

/// Describes one package; [`PackageFixture::build`] turns it into bytes.
#[derive(Debug, Clone)]
pub struct PackageFixture {
    name: PackageName,
    version: PackageVersion,
    skills: Vec<String>,
    files: Vec<(String, Vec<u8>)>,
    moded: Vec<(String, Vec<u8>, u32)>,
    raw: Vec<(Vec<u8>, EntryType, Vec<u8>)>,
    protocols: Vec<String>,
    platforms: Vec<String>,
    skill_contract: u32,
    manifest: Option<Vec<u8>>,
    omit_manifest: bool,
    trailing: Vec<u8>,
    signer: Option<SigningKey>,
    tamper_digest: bool,
}

impl PackageFixture {
    /// A package with no skills, protocol `1.2`, any platform and `skill_contract = 1`.
    ///
    /// # Errors
    /// Returns the validation error when `name` or `version` break their grammar.
    pub fn new(name: &str, version: &str) -> Result<Self, BuildError> {
        Ok(Self {
            name: PackageName::parse(name)?,
            version: PackageVersion::parse(version)?,
            skills: Vec::new(),
            files: Vec::new(),
            moded: Vec::new(),
            raw: Vec::new(),
            protocols: vec!["1.2".to_owned()],
            platforms: Vec::new(),
            skill_contract: 1,
            manifest: None,
            omit_manifest: false,
            trailing: Vec::new(),
            signer: None,
            tamper_digest: false,
        })
    }

    /// Adds `skills/<skill>/skill.toml` and an executable `skills/<skill>/run`.
    #[must_use]
    pub fn skill(mut self, skill: &str) -> Self {
        self.skills.push(skill.to_owned());
        self
    }

    /// Adds a regular file at `path`, relative to the package directory (mode 0644).
    #[must_use]
    pub fn file(mut self, path: &str, bytes: &[u8]) -> Self {
        self.files.push((path.to_owned(), bytes.to_vec()));
        self
    }

    /// Adds a regular file at `path` with an exact archive `mode` (for example one carrying a
    /// setuid or group-write bit, which an installer must ignore).
    #[must_use]
    pub fn file_with_mode(mut self, path: &str, bytes: &[u8], mode: u32) -> Self {
        self.moded.push((path.to_owned(), bytes.to_vec(), mode));
        self
    }

    /// Sets `compat.protocols`.
    #[must_use]
    pub fn protocols(mut self, protocols: &[&str]) -> Self {
        self.protocols = protocols.iter().map(|p| (*p).to_owned()).collect();
        self
    }

    /// Sets `compat.platforms`.
    #[must_use]
    pub fn platforms(mut self, platforms: &[&str]) -> Self {
        self.platforms = platforms.iter().map(|p| (*p).to_owned()).collect();
        self
    }

    /// Sets `compat.skill_contract`.
    #[must_use]
    pub fn skill_contract(mut self, version: u32) -> Self {
        self.skill_contract = version;
        self
    }

    /// Replaces the generated `package.toml` with these exact bytes.
    #[must_use]
    pub fn manifest_bytes(mut self, bytes: &[u8]) -> Self {
        self.manifest = Some(bytes.to_vec());
        self
    }

    /// Leaves `package.toml` out of the archive.
    #[must_use]
    pub fn without_manifest(mut self) -> Self {
        self.omit_manifest = true;
        self
    }

    /// Appends one entry whose full archive path is `name_field`, written byte for byte
    /// (at most 100 bytes), with any entry type: forge `../x`, links, devices, sparse files.
    #[must_use]
    pub fn raw_entry(mut self, name_field: &[u8], kind: EntryType, data: &[u8]) -> Self {
        self.raw.push((name_field.to_vec(), kind, data.to_vec()));
        self
    }

    /// Appends bytes after the tar end marker (inside the compressed stream).
    #[must_use]
    pub fn trailing(mut self, bytes: &[u8]) -> Self {
        self.trailing = bytes.to_vec();
        self
    }

    /// Signs with `key` instead of a fresh random key.
    #[must_use]
    pub fn sign_with(mut self, key: &SigningKey) -> Self {
        self.signer = Some(key.clone());
        self
    }

    /// Signs and records a digest that is not the archive's.
    #[must_use]
    pub fn tamper_digest(mut self) -> Self {
        self.tamper_digest = true;
        self
    }

    /// The `package.toml` text this fixture writes unless overridden.
    fn manifest_text(&self) -> String {
        let list = |items: &[String]| {
            items
                .iter()
                .map(|item| format!("\"{item}\""))
                .collect::<Vec<_>>()
                .join(", ")
        };
        format!(
            "format = 1\nname = \"{}\"\nversion = \"{}\"\ndescription = \"Fixture package\"\n\n[compat]\nprotocols = [{}]\nplatforms = [{}]\nskill_contract = {}\n",
            self.name,
            self.version,
            list(&self.protocols),
            list(&self.platforms),
            self.skill_contract,
        )
    }

    /// The uncompressed tar bytes.
    fn tar_bytes(&self) -> Vec<u8> {
        let mut builder = Builder::new(Vec::new());
        let root = self.name.as_str();
        let mut add = |path: String, mode: u32, data: &[u8]| {
            let mut header = Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(mode);
            header.set_entry_type(EntryType::Regular);
            let _ = builder.append_data(&mut header, path, data);
        };
        if !self.omit_manifest {
            let manifest = self
                .manifest
                .clone()
                .unwrap_or_else(|| self.manifest_text().into_bytes());
            add(format!("{root}/package.toml"), 0o644, &manifest);
        }
        for skill in &self.skills {
            let toml = format!(
                "name = \"{skill}\"\ndescription = \"Fixture skill\"\nrisk = \"read\"\nentry = \"run\"\ntimeout = \"10s\"\nmax_output_bytes = 4096\n"
            );
            add(
                format!("{root}/skills/{skill}/skill.toml"),
                0o644,
                toml.as_bytes(),
            );
            add(
                format!("{root}/skills/{skill}/run"),
                0o755,
                b"#!/bin/sh\necho ran\n",
            );
        }
        for (path, bytes) in &self.files {
            add(format!("{root}/{path}"), 0o644, bytes);
        }
        for (path, bytes, mode) in &self.moded {
            add(format!("{root}/{path}"), *mode, bytes);
        }
        for (name_field, kind, data) in &self.raw {
            let mut header = Header::new_gnu();
            let slot = &mut header.as_old_mut().name;
            let len = name_field.len().min(slot.len());
            slot[..len].copy_from_slice(&name_field[..len]);
            header.set_entry_type(*kind);
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            let _ = builder.append(&header, data.as_slice());
        }
        let mut bytes = builder.into_inner().unwrap_or_default();
        bytes.extend_from_slice(&self.trailing);
        bytes
    }

    /// Builds the archive, signs it and renders the signature file.
    #[must_use]
    pub fn build(self) -> BuiltPackage {
        let archive = compress_to_vec(self.tar_bytes().as_slice(), CompressionLevel::Fastest);
        let archive_sha256 = ArchiveDigest::from_bytes(Sha256::digest(&archive).into());
        let signing_key = self
            .signer
            .clone()
            .unwrap_or_else(|| SigningKey::from_bytes(&rand::random()));
        let public = signing_key.verifying_key().to_bytes();
        let key_id = KeyId::of_public_key(&public);
        let signed_digest = if self.tamper_digest {
            ArchiveDigest::from_bytes(Sha256::digest(b"not the archive").into())
        } else {
            archive_sha256
        };
        let message = signing_message(&self.name, &self.version, &signed_digest, &key_id);
        let signature = STANDARD.encode(signing_key.sign(&message).to_bytes());
        let sig_toml = format!(
            "format = {SIGNATURE_FORMAT}\nalgorithm = \"{SIGNATURE_ALGORITHM}\"\nkey_id = \"{key_id}\"\npackage = \"{}\"\nversion = \"{}\"\narchive_sha256 = \"{signed_digest}\"\nsignature = \"{signature}\"\n",
            self.name, self.version,
        );
        BuiltPackage {
            archive,
            sig_toml,
            signing_key,
            key_id,
            public_key_b64: STANDARD.encode(public),
            archive_sha256,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read as _;

    use domovoi_core::package::{ArchiveDigest, SignatureFile, signing_message};
    use ed25519_dalek::{Signature, SigningKey};
    use ruzstd::decoding::StreamingDecoder;
    use sha2::{Digest as _, Sha256};
    use tar::EntryType;

    use super::PackageFixture;

    fn unpack(archive: &[u8]) -> Vec<(String, u32)> {
        let mut decoder = StreamingDecoder::new(archive).unwrap();
        let mut tar_bytes = Vec::new();
        decoder.read_to_end(&mut tar_bytes).unwrap();
        let mut tar = tar::Archive::new(tar_bytes.as_slice());
        tar.entries()
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                let path = String::from_utf8_lossy(&entry.path_bytes()).into_owned();
                (path, entry.header().mode().unwrap())
            })
            .collect()
    }

    #[test]
    fn built_package_round_trips_and_verifies() {
        let built = PackageFixture::new("nginx-tools", "1.2.0")
            .unwrap()
            .skill("nginx_reload")
            .build();
        let entries = unpack(&built.archive);
        assert!(entries.contains(&("nginx-tools/package.toml".to_owned(), 0o644)));
        assert!(entries.contains(&("nginx-tools/skills/nginx_reload/run".to_owned(), 0o755)));

        let sig: SignatureFile = toml::from_str(&built.sig_toml).unwrap();
        assert_eq!(sig.key_id, built.key_id);
        assert_eq!(sig.archive_sha256, built.archive_sha256);
        assert_eq!(
            built.archive_sha256,
            ArchiveDigest::from_bytes(Sha256::digest(&built.archive).into())
        );
        let bytes = base64_decode(&sig.signature);
        let signature = Signature::from_slice(&bytes).unwrap();
        let message = signing_message(&sig.package, &sig.version, &sig.archive_sha256, &sig.key_id);
        built
            .signing_key
            .verifying_key()
            .verify_strict(&message, &signature)
            .unwrap();
    }

    fn base64_decode(text: &str) -> Vec<u8> {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD
            .decode(text)
            .unwrap()
    }

    #[test]
    fn tampered_digest_differs_from_archive_digest() {
        let built = PackageFixture::new("pkg-a", "1.0.0")
            .unwrap()
            .tamper_digest()
            .build();
        let sig: SignatureFile = toml::from_str(&built.sig_toml).unwrap();
        assert_ne!(sig.archive_sha256, built.archive_sha256);
    }

    #[test]
    fn raw_entry_keeps_a_parent_traversal_name() {
        let built = PackageFixture::new("pkg-a", "1.0.0")
            .unwrap()
            .raw_entry(b"../escape", EntryType::Regular, b"x")
            .build();
        let entries = unpack(&built.archive);
        assert!(entries.iter().any(|(path, _)| path == "../escape"));
    }

    #[test]
    fn sign_with_uses_the_given_key() {
        let key = SigningKey::from_bytes(&[7u8; 32]);
        let built = PackageFixture::new("pkg-a", "1.0.0")
            .unwrap()
            .sign_with(&key)
            .build();
        assert_eq!(built.signing_key.to_bytes(), key.to_bytes());
    }
}
