//! Signature file, trust, signature, digest, identity and compatibility checks.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use domovoi_config::{PackagesSection, TrustedKey};
use domovoi_core::package::{
    KeyId, KeyState, PackageFailureCode as Code, PackageName, PackageVersion,
};
use domovoi_testkit::{BuiltPackage, PackageFixture};
use ed25519_dalek::SigningKey;

use super::support::code_of;
use crate::packages::keys::KeySet;
use crate::packages::verify::{
    check_digest, compat, manifest_identity, parse_manifest, parse_signature_file, same_identity,
    sha256_stream, signature,
};

fn section_trusting(built: &BuiltPackage) -> PackagesSection {
    PackagesSection {
        allow_install: true,
        revoked: Vec::new(),
        trusted_keys: vec![TrustedKey {
            id: built.key_id.to_string(),
            public_key: built.public_key_b64.clone(),
            label: None,
        }],
        limits: domovoi_config::PackageLimits::default(),
    }
}

fn built() -> BuiltPackage {
    PackageFixture::new("pkg-a", "1.0.0").unwrap().build()
}

#[test]
fn agent_accepts_a_trusted_signature() {
    let built = built();
    let keys = KeySet::from_section(&section_trusting(&built)).unwrap();
    let file = parse_signature_file(built.sig_toml.as_bytes()).unwrap();
    let key = keys.lookup(&file.key_id).unwrap();
    signature(key, &file).unwrap();
    check_digest(&sha256_stream(built.archive.as_slice()).unwrap(), &file).unwrap();
    assert_eq!(keys.state(&built.key_id), KeyState::Trusted);
}

#[test]
fn agent_rejects_untrusted_signature() {
    let built = built();
    let other = PackageFixture::new("pkg-b", "1.0.0").unwrap().build();
    let keys = KeySet::from_section(&section_trusting(&other)).unwrap();
    assert_eq!(code_of(keys.lookup(&built.key_id)), Code::UntrustedKey);
    assert_eq!(keys.state(&built.key_id), KeyState::Unknown);
}

#[test]
fn agent_rejects_revoked_key_even_when_trusted() {
    let built = built();
    let mut section = section_trusting(&built);
    section.revoked.push(built.key_id.to_string());
    let keys = KeySet::from_section(&section).unwrap();
    assert_eq!(code_of(keys.lookup(&built.key_id)), Code::RevokedKey);
    assert_eq!(keys.state(&built.key_id), KeyState::Revoked);
}

#[test]
fn agent_rejects_bad_signature() {
    let built = built();
    let keys = KeySet::from_section(&section_trusting(&built)).unwrap();
    let mut file = parse_signature_file(built.sig_toml.as_bytes()).unwrap();
    let key = keys.lookup(&file.key_id).unwrap();

    let mut raw = STANDARD.decode(&file.signature).unwrap();
    raw[0] ^= 1;
    file.signature = STANDARD.encode(&raw);
    assert_eq!(code_of(signature(key, &file)), Code::BadSignature);

    let mut other_version = parse_signature_file(built.sig_toml.as_bytes()).unwrap();
    other_version.version = PackageVersion::parse("2.0.0").unwrap();
    assert_eq!(code_of(signature(key, &other_version)), Code::BadSignature);
}

#[test]
fn agent_rejects_identity_mismatch() {
    let built = built();
    let file = parse_signature_file(built.sig_toml.as_bytes()).unwrap();
    let name = PackageName::parse("pkg-a").unwrap();
    let version = PackageVersion::parse("1.0.0").unwrap();
    same_identity(&file, &name, &version).unwrap();
    let other_name = PackageName::parse("pkg-b").unwrap();
    let other_version = PackageVersion::parse("1.0.1").unwrap();
    assert_eq!(
        code_of(same_identity(&file, &other_name, &version)),
        Code::IdentityMismatch
    );
    assert_eq!(
        code_of(same_identity(&file, &name, &other_version)),
        Code::IdentityMismatch
    );

    let manifest = parse_manifest(
        b"format = 1\nname = \"pkg-b\"\nversion = \"1.0.0\"\n[compat]\nprotocols = [\"1.2\"]\nskill_contract = 1\n",
    )
    .unwrap();
    assert_eq!(
        code_of(manifest_identity(&manifest, &name, &version)),
        Code::IdentityMismatch
    );
}

#[test]
fn agent_rejects_a_tampered_digest() {
    let built = PackageFixture::new("pkg-a", "1.0.0")
        .unwrap()
        .tamper_digest()
        .build();
    let file = parse_signature_file(built.sig_toml.as_bytes()).unwrap();
    let actual = sha256_stream(built.archive.as_slice()).unwrap();
    assert_eq!(code_of(check_digest(&actual, &file)), Code::DigestMismatch);
}

#[test]
fn signature_file_is_strict() {
    let text = built().sig_toml;
    let invalid = |bytes: &[u8]| code_of(parse_signature_file(bytes));
    assert_eq!(invalid(&vec![b' '; 17 * 1024]), Code::SignatureFileInvalid);
    assert_eq!(
        invalid(format!("{text}extra = 1\n").as_bytes()),
        Code::SignatureFileInvalid
    );
    let bad_base64 = text.replace("signature = \"", "signature = \"!!");
    assert_eq!(invalid(bad_base64.as_bytes()), Code::SignatureFileInvalid);
    let line = text.lines().find(|l| l.starts_with("signature")).unwrap();
    let short = text.replace(
        line,
        &format!("signature = \"{}\"", STANDARD.encode([0_u8; 63])),
    );
    assert_eq!(invalid(short.as_bytes()), Code::SignatureFileInvalid);
    let format = text.replace("format = 1", "format = 2");
    assert_eq!(invalid(format.as_bytes()), Code::SignatureFileInvalid);
    let algorithm = text.replace("ed25519", "rsa");
    assert_eq!(invalid(algorithm.as_bytes()), Code::SignatureFileInvalid);
}

#[test]
fn compat_rules_are_enforced() {
    let manifest = |protocols: &str, platforms: &str, contract: u32| {
        let text = format!(
            "format = 1\nname = \"pkg-a\"\nversion = \"1.0.0\"\n[compat]\nprotocols = [{protocols}]\nplatforms = [{platforms}]\nskill_contract = {contract}\n"
        );
        parse_manifest(text.as_bytes()).unwrap()
    };
    let here = "linux-x86_64";
    compat(&manifest("\"1.2\"", "", 1), "1.2", here).unwrap();
    compat(&manifest("\"1.2\"", "\"linux-x86_64\"", 1), "1.2", here).unwrap();
    for refused in [
        manifest("\"1.1\"", "", 1),
        manifest("\"1.2\"", "\"linux-aarch64\"", 1),
        manifest("\"1.2\"", "", 2),
    ] {
        assert_eq!(code_of(compat(&refused, "1.2", here)), Code::Incompatible);
    }
}

#[test]
fn manifest_parsing_is_strict() {
    assert_eq!(
        code_of(parse_manifest(b"format = 1\nbogus = 2\n")),
        Code::ManifestInvalid
    );
    assert_eq!(
        code_of(parse_manifest(&vec![b'#'; 70 * 1024])),
        Code::ManifestInvalid
    );
}

#[test]
fn trusted_key_id_must_match_public_key() {
    let built = built();
    let mut section = section_trusting(&built);
    section.trusted_keys[0].id = "0000000000000000".to_owned();
    assert!(KeySet::from_section(&section).is_err());

    let mut garbled = section_trusting(&built);
    garbled.trusted_keys[0].public_key = "not base64!".to_owned();
    assert!(KeySet::from_section(&garbled).is_err());

    let mut short = section_trusting(&built);
    short.trusted_keys[0].public_key = STANDARD.encode([1_u8; 31]);
    assert!(KeySet::from_section(&short).is_err());

    let public = SigningKey::from_bytes(&[9_u8; 32])
        .verifying_key()
        .to_bytes();
    let good = PackagesSection {
        trusted_keys: vec![TrustedKey {
            id: KeyId::of_public_key(&public).to_string(),
            public_key: STANDARD.encode(public),
            label: None,
        }],
        ..section_trusting(&built)
    };
    assert_eq!(KeySet::from_section(&good).unwrap().trusted_count(), 1);
}
