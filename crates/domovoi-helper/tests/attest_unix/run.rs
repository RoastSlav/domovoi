use std::sync::{Arc, Mutex};

use domovoi_core::attest::{Report, parse_report, signing_bytes};
use domovoi_helper::attest::AttestStage;
use domovoi_helper::attest::state::StateFile;
use domovoi_helper::attest::{AttestOutcome, safety_attest};
use domovoi_helper::state::WriteFaults;
use ed25519_dalek::{Signature, VerifyingKey};

use crate::world::{BOOT_B, World, boot, generation, nonce, reason, verify};

fn read_state(world: &World) -> StateFile {
    StateFile::parse(&std::fs::read(world.path("state")).expect("state")).expect("valid state")
}

#[tokio::test]
async fn a_complete_report_verifies_under_the_signing_prefix_and_names_the_challenge() {
    let world = World::new();
    world.init();
    let attestation = world.signed(7).await;
    let Report::Complete(report) = verify(&attestation) else {
        panic!("expected a complete report");
    };
    assert_eq!(report.nonce, nonce(7));
    assert_eq!(report.generation, 1);
    assert_eq!(report.boot_id, boot(crate::world::BOOT_A));
    assert_eq!(report.computed_at.to_string(), "2026-07-01T12:00:00Z");
    assert_eq!(report.helper_key_id.len(), 16);
    assert!(report.check_hashes().is_ok());
    assert_eq!(report.routes.hub_addr, ["192.0.2.77"]);
    assert_eq!(report.config.cidr, ["198.51.100.0/24"]);
    for unit in domovoi_core::attest::MANDATORY_UNITS {
        assert!(
            report.config.protected_unit.iter().any(|u| u == unit),
            "{unit}"
        );
    }

    // A signature over the bare report, without the domain prefix, is not an attestation.
    let key = VerifyingKey::from_bytes(&attestation.public_key).expect("key");
    let signature = Signature::from_bytes(&attestation.signature);
    assert!(key.verify_strict(&attestation.report, &signature).is_err());
    assert!(
        key.verify_strict(&signing_bytes(&attestation.report), &signature)
            .is_ok()
    );
}

#[tokio::test]
async fn state_is_durable_before_the_report_is_signed() {
    let world = World::new();
    world.init();
    let events: Arc<Mutex<Vec<(AttestStage, u64)>>> = Arc::default();
    let mut env = world.env();
    let seen = events.clone();
    let state_path = world.path("state");
    env.stage_hook = Some(Arc::new(move |stage| {
        let on_disk = StateFile::parse(&std::fs::read(&state_path).expect("state"))
            .expect("valid")
            .generation;
        seen.lock().expect("lock").push((stage, on_disk));
    }));
    let AttestOutcome::Signed(attestation) = safety_attest(&env, nonce(1)).await else {
        panic!("signed");
    };
    assert_eq!(generation(&attestation), 1);
    assert_eq!(
        *events.lock().expect("lock"),
        [(AttestStage::StateWritten, 1), (AttestStage::Signed, 1)],
        "the new generation is on disk when the signature is made, and not before"
    );
}

#[tokio::test]
async fn state_write_failed_is_signed_incomplete_without_generation_change() {
    for faults in [
        WriteFaults {
            temp_sync: true,
            ..WriteFaults::default()
        },
        WriteFaults {
            rename: true,
            ..WriteFaults::default()
        },
    ] {
        let world = World::new();
        world.init();
        let mut env = world.env();
        env.write_faults = faults;
        let AttestOutcome::Signed(attestation) = safety_attest(&env, nonce(1)).await else {
            panic!("signed incomplete");
        };
        assert_eq!(reason(&attestation), "state_write_failed");
        assert_eq!(
            read_state(&world).generation,
            0,
            "no generation change is visible"
        );
        assert!(
            !world.path(".state.tmp").exists(),
            "the temporary file is cleaned up"
        );
        assert_eq!(
            generation(&world.signed(2).await),
            1,
            "the next honest attestation succeeds"
        );
    }
}

#[tokio::test]
async fn interrupted_state_write_never_reuses_generation() {
    let world = World::new();
    world.init();
    let mut env = world.env();
    // The new state reached the disk but its directory could not be synced.
    env.write_faults = WriteFaults {
        dir_sync: true,
        ..WriteFaults::default()
    };
    let AttestOutcome::Signed(failed) = safety_attest(&env, nonce(1)).await else {
        panic!("signed incomplete");
    };
    assert_eq!(reason(&failed), "state_write_failed");
    // The hub saw no generation 1; the helper must still never hand out generation 1 for
    // different hashes. The same inputs give the same generation, a change gives the next.
    assert_eq!(generation(&world.signed(2).await), 1);
    let mut next = world.sharing();
    next.text
        .helper_toml
        .push_str("protected_units = [\"nginx\"]\n");
    assert_eq!(generation(&next.signed(3).await), 2);
}

#[tokio::test]
async fn leftover_state_tmp_is_deleted_under_the_lock() {
    let world = World::new();
    world.init();
    std::fs::write(world.path(".state.tmp"), b"half a state").expect("leftover");
    assert_eq!(generation(&world.signed(1).await), 1);
    assert!(!world.path(".state.tmp").exists());
}

#[tokio::test]
async fn config_changes_move_the_generation_and_a_revert_moves_it_again() {
    let world = World::new();
    world.init();
    assert_eq!(generation(&world.signed(1).await), 1);
    let mut changed = world.sharing();
    changed
        .text
        .helper_toml
        .push_str("protected_containers = [\"app-a\"]\n");
    assert_eq!(generation(&changed.signed(2).await), 2);
    let reverted = world.sharing();
    assert_eq!(
        generation(&reverted.signed(3).await),
        3,
        "a revert is a change too"
    );
    assert_eq!(generation(&reverted.signed(4).await), 3);
}

#[tokio::test]
async fn a_new_boot_id_alone_keeps_the_generation_and_is_recorded() {
    let world = World::new();
    world.init();
    assert_eq!(generation(&world.signed(1).await), 1);
    let mut rebooted = world.sharing();
    rebooted.text.boot_id = format!("{BOOT_B}\n");
    let attestation = rebooted.signed(2).await;
    assert_eq!(
        generation(&attestation),
        1,
        "a reboot is not a safety-set change"
    );
    let Report::Complete(report) = verify(&attestation) else {
        panic!("complete");
    };
    assert_eq!(report.boot_id, boot(BOOT_B));
    assert_eq!(read_state(&world).boot_id, boot(BOOT_B));
}

#[tokio::test]
async fn oversize_report_fails_closed() {
    let world = World::new();
    world.init();
    let mut big = world.sharing();
    let names: Vec<String> = (0..4000).map(|n| format!("\"container-{n:05}\"")).collect();
    big.text.helper_toml = format!("[safety]\nprotected_containers = [{}]\n", names.join(", "));
    let attestation = big.signed(1).await;
    assert_eq!(reason(&attestation), "report_too_large");
    assert!(
        attestation.report.len() < 1024,
        "the incomplete report is minimal"
    );
    assert_eq!(read_state(&world).generation, 0);
}

#[tokio::test]
async fn config_problems_are_signed_incomplete_with_their_reason() {
    for (toml, expected) in [
        (
            "[safety]\nmanagement_cidrs = [\"not-a-cidr\"]\n",
            "config_invalid",
        ),
        ("[safety.containment]\nmax_ttl = \"0s\"\n", "config_invalid"),
        ("[safety]\nunknown_key = 1\n", "config_invalid"),
        (
            "[safety]\nprotected_units = [\"a\\\\b.service\"]\n",
            "config_invalid",
        ),
        ("this is [ not toml", "config_unreadable"),
    ] {
        let world = World::new();
        world.init();
        let mut broken = world.sharing();
        broken.text.helper_toml = toml.to_owned();
        let attestation = broken.signed(1).await;
        assert_eq!(reason(&attestation), expected, "{toml}");
        assert_eq!(
            read_state(&world).generation,
            0,
            "an incomplete report changes no state"
        );
    }
    let world = World::new();
    world.init();
    let mut no_agent = world.sharing();
    no_agent.text.agent_toml = "[agent]\nhost_name = \"a\"\n".to_owned();
    assert_eq!(reason(&no_agent.signed(1).await), "config_unreadable");
}

#[tokio::test]
async fn a_hub_url_that_names_a_host_yields_a_complete_report_without_hub_addr() {
    let world = World::new();
    world.init();
    let mut named = world.sharing();
    named.text.agent_toml =
        "[agent]\nhost_name = \"a\"\nhub_url = \"wss://hub.example.net:7421\"\n".to_owned();
    let attestation = named.signed(1).await;
    let Report::Complete(report) = verify(&attestation) else {
        panic!("complete");
    };
    assert!(report.routes.hub_addr.is_empty());
    let text = String::from_utf8(attestation.report.clone()).expect("utf-8");
    assert!(!text.contains("hub_addr="));
    assert!(parse_report(&attestation.report).is_ok());
}

#[tokio::test]
async fn state_file_has_exact_format_checksum_and_modes() {
    let world = World::new();
    world.init();
    let attestation = world.signed(1).await;
    assert_eq!(generation(&attestation), 1);
    let text = std::fs::read_to_string(world.path("state")).expect("state");
    let keys: Vec<&str> = text
        .lines()
        .map(|line| line.split('=').next().unwrap_or(""))
        .collect();
    assert_eq!(
        keys,
        [
            "format",
            "key_fingerprint",
            "generation",
            "config_hash",
            "route_state_hash",
            "boot_id",
            "checksum"
        ]
    );
    assert!(text.ends_with('\n') && !text.contains('\r'));
    let (body, checksum) = text.split_at(text.rfind("checksum=").expect("checksum line"));
    assert_eq!(
        checksum.trim_end(),
        format!(
            "checksum={}",
            domovoi_core::attest::Hash32::of(body.as_bytes())
        )
    );
    for name in ["state", "attest.key", "state.lock"] {
        assert_eq!(world.mode(name), 0o600, "{name}");
    }
    assert!(
        !world.path(".state.tmp").exists(),
        "the temporary file is renamed away"
    );
}
