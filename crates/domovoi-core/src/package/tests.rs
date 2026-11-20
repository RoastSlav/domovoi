//! Tests for the package grammar, signing message, failure codes and contracts.

use std::collections::BTreeSet;
use std::str::FromStr as _;

use jiff::Timestamp;
use sha2::{Digest as _, Sha256};

use super::*;
use crate::capability::{
    ContractIndex, EffectClass, ExecutionPath, builtin_contracts, package_contracts,
};

fn name(text: &str) -> PackageName {
    PackageName::parse(text).unwrap()
}
fn version(text: &str) -> PackageVersion {
    PackageVersion::parse(text).unwrap()
}
fn digest_of(bytes: &[u8]) -> ArchiveDigest {
    ArchiveDigest::from_bytes(Sha256::digest(bytes).into())
}

#[test]
fn package_name_accepts_and_rejects_by_grammar() {
    for ok in ["nginx-tools", "ab", "a1", "a-b-c", "x".repeat(48).as_str()] {
        assert!(PackageName::parse(ok).is_ok(), "{ok}");
    }
    let long = "x".repeat(49);
    for bad in [
        "",
        "a",
        "1a",
        "-a",
        "a-",
        "a--b",
        "A",
        "a_b",
        "a.b",
        "a b",
        "é",
        long.as_str(),
    ] {
        assert!(PackageName::parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn package_version_accepts_and_rejects_by_grammar() {
    for ok in [
        "0.0.0",
        "1.2.0",
        "10.20.30",
        "1.0.0-rc.1",
        "1.0.0-beta",
        "1.0.0-0a",
    ] {
        assert!(PackageVersion::parse(ok).is_ok(), "{ok}");
    }
    let long = format!("1.0.0-{}", "a".repeat(30));
    for bad in [
        "",
        "1",
        "1.2",
        "1.2.3.4",
        "01.2.3",
        "1.02.3",
        "1.2.x",
        "1.2.3+b",
        "1.2.3-",
        "1.2.3-A",
        "1.2.3-a..b",
        "v1.2.3",
        " 1.2.3",
        long.as_str(),
    ] {
        assert!(PackageVersion::parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn key_id_is_first_sixteen_hex_of_public_key_sha256() {
    let key = [7u8; 32];
    let expected = hex::encode(Sha256::digest(key))[..16].to_owned();
    assert_eq!(KeyId::of_public_key(&key).as_str(), expected);
    // Literal vector: `head -c 32 /dev/zero | sha256sum` starts with 66687aadf862bd77.
    assert_eq!(
        KeyId::of_public_key(&[0u8; 32]).as_str(),
        "66687aadf862bd77"
    );
    assert!(KeyId::parse("66687aadf862bd77").is_ok());
    for bad in [
        "",
        "66687AADF862BD77",
        "66687aadf862bd7",
        "66687aadf862bd777",
        "g6687aadf862bd77",
    ] {
        assert!(KeyId::parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn signing_message_is_byte_exact() {
    let digest = ArchiveDigest::from_bytes([0xab; 32]);
    let key = KeyId::parse("a1b2c3d4e5f60718").unwrap();
    let message = signing_message(&name("nginx-tools"), &version("1.2.0"), &digest, &key);
    let expected = format!(
        "domovoi-package-v1\nnginx-tools\n1.2.0\n{}\na1b2c3d4e5f60718\n",
        "ab".repeat(32)
    );
    assert_eq!(message, expected.into_bytes());
    assert_eq!(message.last(), Some(&b'\n'));
}

fn sig_toml(extra: &str, format: u32, algorithm: &str) -> String {
    format!(
        "format = {format}\nalgorithm = \"{algorithm}\"\nkey_id = \"a1b2c3d4e5f60718\"\n\
         package = \"nginx-tools\"\nversion = \"1.2.0\"\narchive_sha256 = \"{}\"\n\
         signature = \"AAAA\"\n{extra}",
        "00".repeat(32)
    )
}

#[test]
fn signature_file_rejects_unknown_field_wrong_format_and_wrong_algorithm() {
    let good: SignatureFile = toml::from_str(&sig_toml("", 1, "ed25519")).unwrap();
    assert_eq!(good.format, SIGNATURE_FORMAT);
    assert_eq!(good.algorithm, SIGNATURE_ALGORITHM);
    assert!(toml::from_str::<SignatureFile>(&sig_toml("extra = 1\n", 1, "ed25519")).is_err());
    // Format and algorithm parse; the agent compares them to the constants.
    let wrong_format: SignatureFile = toml::from_str(&sig_toml("", 2, "ed25519")).unwrap();
    assert_ne!(wrong_format.format, SIGNATURE_FORMAT);
    let wrong_algorithm: SignatureFile = toml::from_str(&sig_toml("", 1, "rsa")).unwrap();
    assert_ne!(wrong_algorithm.algorithm, SIGNATURE_ALGORITHM);
    let bad_name = sig_toml("", 1, "ed25519").replace("nginx-tools", "Nginx");
    assert!(toml::from_str::<SignatureFile>(&bad_name).is_err());
}

#[test]
fn package_manifest_is_strict() {
    let text = "format = 1\nname = \"nginx-tools\"\nversion = \"1.2.0\"\n[compat]\nprotocols = [\"1.2\"]\nskill_contract = 1\n";
    let manifest: PackageManifest = toml::from_str(text).unwrap();
    assert!(manifest.compat.platforms.is_empty());
    assert!(toml::from_str::<PackageManifest>(&format!("{text}run_as = \"root\"\n")).is_err());
}

#[test]
fn failure_codes_are_unique_and_round_trip() {
    let mut seen = BTreeSet::new();
    for code in PackageFailureCode::ALL {
        assert!(seen.insert(code.as_str()), "{code}");
        assert_eq!(PackageFailureCode::from_str(code.as_str()).unwrap(), *code);
        let json = serde_json::to_string(code).unwrap();
        assert_eq!(json, format!("\"{}\"", code.as_str()));
    }
    assert_eq!(PackageFailureCode::ALL.len(), 35);
    assert!(PackageFailureCode::from_str("nope").is_err());
}

#[test]
fn package_list_entry_snapshot() {
    let entry = PackageListEntry {
        name: name("nginx-tools"),
        version: version("1.2.0"),
        key_id: KeyId::parse("a1b2c3d4e5f60718").unwrap(),
        archive_sha256: digest_of(b"archive"),
        installed_at: Timestamp::from_second(1_700_000_000).unwrap(),
        skills: vec!["nginx_reload".to_owned()],
        status: PackageStatus::DisabledUntrustedKey,
        key_state: KeyState::Unknown,
        executable: false,
        executable_reason: Some("skill_runner_not_integrated".to_owned()),
    };
    insta::assert_json_snapshot!(entry);
}

#[test]
fn package_contracts_validate_and_index() {
    let contracts = package_contracts();
    let ids: Vec<&str> = contracts.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "agent.package.install",
            "agent.package.uninstall",
            "agent.package.list"
        ]
    );
    for contract in &contracts {
        contract.validate().unwrap();
        assert_eq!(contract.execution, ExecutionPath::Agent);
        assert_eq!(contract.input_schema["additionalProperties"], false);
    }
    assert_eq!(contracts[0].effect, EffectClass::Mutate);
    assert_eq!(contracts[2].effect, EffectClass::Read);
    let mut all = builtin_contracts();
    all.extend(contracts);
    ContractIndex::new(all).unwrap();
}

#[test]
fn builtin_contracts_are_unchanged_by_package_contracts() {
    let ids: Vec<String> = builtin_contracts()
        .iter()
        .map(|c| c.id.to_string())
        .collect();
    assert_eq!(ids.len(), 10);
    assert!(ids.iter().all(|id| !id.starts_with("agent.package.")));
}
