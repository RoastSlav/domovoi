//! Signed report vectors under a published test-only seed, the domain separation of the three
//! prefixes, and the state file fixtures.

use domovoi_core::KeyFingerprint;
use domovoi_core::attest::{
    BootId, CONFIG_PREFIX, Hash32, ROUTES_PREFIX, Report, SIGNING_PREFIX, ZERO_HASH, domain_bytes,
    parse_report, render_complete, render_incomplete, signing_bytes,
};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};

use crate::common::{
    BOOT, TEST_SEED, complete_report, incomplete_report, parse_uint_text, vector_bytes,
    vector_path, vector_text,
};

fn key() -> SigningKey {
    SigningKey::from_bytes(&TEST_SEED)
}

fn sign(report: &[u8]) -> Signature {
    key().sign(&signing_bytes(report))
}

fn signature_from(name: &str) -> Signature {
    let bytes: [u8; 64] = hex::decode(vector_text(name).trim_end())
        .expect("hex")
        .try_into()
        .expect("64 bytes");
    Signature::from_bytes(&bytes)
}

fn public() -> VerifyingKey {
    key().verifying_key()
}

fn state_with_checksum(body: &str) -> String {
    format!("{body}checksum={}\n", Hash32::of(body.as_bytes()))
}

fn state_body(generation: &str, config: &Hash32, routes: &Hash32, fingerprint: &str) -> String {
    format!(
        "format=1\nkey_fingerprint={fingerprint}\ngeneration={generation}\nconfig_hash={config}\nroute_state_hash={routes}\nboot_id={BOOT}\n"
    )
}

fn fingerprint() -> String {
    KeyFingerprint::of_public_key(&public().to_bytes()).to_string()
}

/// The state fixtures: name and text. The first two are valid, the rest each break one rule.
fn state_fixtures() -> Vec<(&'static str, String)> {
    let report = complete_report();
    let (config, routes) = (report.config_hash, report.route_state_hash);
    let fp = fingerprint();
    let valid = state_with_checksum(&state_body("1", &config, &routes, &fp));
    let init = state_with_checksum(&state_body("0", &ZERO_HASH, &ZERO_HASH, &fp));
    let flip = |text: &str, from: &str, to: &str| text.replacen(from, to, 1);
    vec![
        ("state_valid.txt", valid.clone()),
        ("state_init.txt", init),
        (
            "state_bad_checksum.txt",
            flip(&valid, "generation=1", "generation=2"),
        ),
        (
            "state_bad_format.txt",
            state_with_checksum(&flip(
                &state_body("1", &config, &routes, &fp),
                "format=1",
                "format=2",
            )),
        ),
        (
            "state_leading_zero_generation.txt",
            state_with_checksum(&state_body("01", &config, &routes, &fp)),
        ),
        (
            "state_uppercase_hash.txt",
            state_with_checksum(&state_body("1", &config, &routes, &fp.to_uppercase())),
        ),
        ("state_extra_line.txt", format!("{valid}extra=1\n")),
        ("state_no_final_lf.txt", valid.trim_end().to_owned()),
        (
            "state_keys_out_of_order.txt",
            state_with_checksum(&format!(
                "format=1\ngeneration=1\nkey_fingerprint={fp}\nconfig_hash={config}\nroute_state_hash={routes}\nboot_id={BOOT}\n"
            )),
        ),
        ("state_empty.txt", String::new()),
    ]
}

/// A strict reading of the state file layout, enough to classify the fixtures.
fn check_state(bytes: &[u8]) -> Result<(), &'static str> {
    let text = std::str::from_utf8(bytes).map_err(|_| "utf-8")?;
    let body = text.strip_suffix('\n').ok_or("final LF")?;
    let lines: Vec<&str> = body.split('\n').collect();
    let keys = [
        "format",
        "key_fingerprint",
        "generation",
        "config_hash",
        "route_state_hash",
        "boot_id",
        "checksum",
    ];
    if lines.len() != keys.len() {
        return Err("line count");
    }
    let mut values = Vec::new();
    for (line, key) in lines.iter().zip(keys) {
        let value = line
            .strip_prefix(key)
            .and_then(|rest| rest.strip_prefix('='))
            .ok_or("key order")?;
        values.push(value);
    }
    if values[0] != "1" {
        return Err("format");
    }
    values[1].parse::<Hash32>().map_err(|_| "fingerprint")?;
    parse_uint_text(values[2]).ok_or("generation")?;
    values[3].parse::<Hash32>().map_err(|_| "config_hash")?;
    values[4]
        .parse::<Hash32>()
        .map_err(|_| "route_state_hash")?;
    BootId::parse(values[5]).map_err(|_| "boot_id")?;
    let before = &text[..text.len() - lines[6].len() - 1];
    if values[6] != Hash32::of(before.as_bytes()).to_string() {
        return Err("checksum");
    }
    Ok(())
}

/// Regenerates every signed and state fixture. Run only to change a frozen vector on purpose:
/// `cargo test -p domovoi-core --test attest -- --ignored write_signed_vectors`.
#[test]
#[ignore = "rewrites the frozen fixtures"]
fn write_signed_vectors() {
    let write = |name: &str, text: &str| std::fs::write(vector_path(name), text).expect("write");
    write("test_seed.hex", &format!("{}\n", hex::encode(TEST_SEED)));
    write(
        "test_public_key.hex",
        &format!("{}\n", hex::encode(public().to_bytes())),
    );
    let complete = render_complete(&complete_report());
    write(
        "report_complete.txt",
        &String::from_utf8(complete.clone()).expect("utf-8"),
    );
    write(
        "report_complete.sig",
        &format!("{}\n", hex::encode(sign(&complete).to_bytes())),
    );
    let incomplete = render_incomplete(&incomplete_report());
    write(
        "report_incomplete.txt",
        &String::from_utf8(incomplete.clone()).expect("utf-8"),
    );
    write(
        "report_incomplete.sig",
        &format!("{}\n", hex::encode(sign(&incomplete).to_bytes())),
    );
    for (name, text) in state_fixtures() {
        write(name, &text);
    }
}

#[test]
fn signature_vector_is_deterministic_for_the_test_seed() {
    assert_eq!(
        vector_text("test_seed.hex").trim_end(),
        hex::encode(TEST_SEED)
    );
    assert_eq!(
        vector_text("test_public_key.hex").trim_end(),
        hex::encode(public().to_bytes())
    );
    for (report, bytes, signature) in [
        (
            "complete",
            render_complete(&complete_report()),
            "report_complete.sig",
        ),
        (
            "incomplete",
            render_incomplete(&incomplete_report()),
            "report_incomplete.sig",
        ),
    ] {
        assert_eq!(
            vector_bytes(&format!("report_{report}.txt")),
            bytes,
            "{report} bytes"
        );
        assert_eq!(
            sign(&bytes),
            signature_from(signature),
            "{report} signature"
        );
        public()
            .verify_strict(&signing_bytes(&bytes), &signature_from(signature))
            .unwrap_or_else(|err| panic!("{report}: {err}"));
        // Signing twice gives the same bytes: the vector is deterministic.
        assert_eq!(sign(&bytes).to_bytes(), sign(&bytes).to_bytes());
    }
}

#[test]
fn signed_report_vectors_parse_and_recompute_their_hashes() {
    let Ok(Report::Complete(report)) = parse_report(&vector_bytes("report_complete.txt")) else {
        panic!("complete");
    };
    report.check_hashes().expect("hashes");
    assert_eq!(report.generation, 1);
    assert_eq!(report.cidrs().len(), 2);
    assert_eq!(report.hub_addrs().len(), 1);
    assert!(report.link_peers().is_empty());
    let Ok(Report::Incomplete(incomplete)) = parse_report(&vector_bytes("report_incomplete.txt"))
    else {
        panic!("incomplete");
    };
    assert_eq!(incomplete.reason.as_str(), "state_missing");
}

#[test]
fn bare_report_signature_does_not_verify() {
    let report = render_complete(&complete_report());
    let bare = key().sign(&report);
    assert!(
        public()
            .verify_strict(&signing_bytes(&report), &bare)
            .is_err()
    );
    assert!(
        public().verify_strict(&report, &bare).is_ok(),
        "the bare signature is valid only for the bare bytes"
    );
    let prefixed = sign(&report);
    assert!(public().verify_strict(&report, &prefixed).is_err());
    // A signature over another domain's bytes is not a report signature either.
    let config = domain_bytes(CONFIG_PREFIX, &complete_report().config.lines());
    let over_config = key().sign(&config);
    assert!(
        public()
            .verify_strict(&signing_bytes(&config), &over_config)
            .is_err()
    );
    // Any change to the report breaks the signature.
    let mut changed = report.clone();
    changed[0] ^= 1;
    assert!(
        public()
            .verify_strict(&signing_bytes(&changed), &prefixed)
            .is_err()
    );
}

#[test]
fn the_three_domain_prefixes_are_distinct_and_end_in_lf() {
    let prefixes = [SIGNING_PREFIX, CONFIG_PREFIX, ROUTES_PREFIX];
    for (index, prefix) in prefixes.iter().enumerate() {
        assert!(prefix.ends_with('\n') && prefix.starts_with("domovoi-safety-"));
        for other in &prefixes[index + 1..] {
            assert_ne!(prefix, other);
            assert!(!prefix.starts_with(other) && !other.starts_with(prefix));
        }
    }
    assert_eq!(signing_bytes(b"x"), b"domovoi-safety-attest-v1\nx");
}

#[test]
fn state_fixtures_are_frozen_and_classified() {
    for (name, text) in state_fixtures() {
        assert_eq!(vector_bytes(name), text.as_bytes(), "{name}");
        let valid = matches!(name, "state_valid.txt" | "state_init.txt");
        assert_eq!(check_state(text.as_bytes()).is_ok(), valid, "{name}");
    }
    // The initial state is generation 0 with zero hashes (the first real attestation is 1).
    let init = vector_text("state_init.txt");
    assert!(init.contains("generation=0\n"));
    assert!(init.contains(&format!(
        "config_hash={ZERO_HASH}\nroute_state_hash={ZERO_HASH}\n"
    )));
}
