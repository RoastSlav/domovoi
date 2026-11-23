//! G1-G2 (generation), H1-H3 (state file, fail-closed conditions, atomic write).

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;

use common::{BOOT_A, BOOT_B, HOST, Rig, T0, env};
use r10::fixture::{FixtureSource, cidr};
use r10::helper::{ContainRequest, HelperRuntime, Inputs, Refused, apply_contain};
use r10::hash::sha256;
use r10::hub::{Outcome, Rejection};
use r10::state::HelperState;

fn mode(p: &std::path::Path) -> u32 {
    fs::metadata(p).unwrap().permissions().mode() & 0o777
}

fn report(rig: &Rig, now: u64, boot: &str) -> String {
    String::from_utf8(rig.attest_raw([4; 32], now, boot).unwrap().report).unwrap()
}

fn gen_of(rep: &str) -> u64 {
    rep.lines().find_map(|l| l.strip_prefix("generation=")).unwrap().parse().unwrap()
}

fn bump(rig: &mut Rig, to: u32) {
    for r in &mut rig.snap.routes {
        if r.dst == cidr("192.168.1.0/24") {
            r.metric = to;
        }
    }
}

#[test]
fn containment_membership_change_keeps_generation() {
    let mut rig = Rig::new();
    let g0 = gen_of(&report(&rig, T0, BOOT_A));
    let src = FixtureSource(rig.snap.clone());
    let e = env(T0, BOOT_A);
    let inp = Inputs { config_text: Some(&rig.toml), routes: &src, env: &e };
    let mut rt = HelperRuntime { per_hour: 10, ..Default::default() };
    let req = ContainRequest { target: "203.0.113.0/24".into(), ttl_s: 60, expected: None, agent_max_ttl: None, agent_max_active: None, agent_reported_active: None };
    apply_contain(&rig.dir, &req, &inp, &mut rt, &mut |_| {}, &mut |_| {}).expect("applied");
    assert_eq!(rt.nft.len(), 1);
    assert_eq!(gen_of(&report(&rig, T0 + 1, BOOT_A)), g0, "nft set membership is outside the safety set");
    rt.nft.clear();
    assert_eq!(gen_of(&report(&rig, T0 + 2, BOOT_A)), g0);
    bump(&mut rig, 77);
    assert_eq!(gen_of(&report(&rig, T0 + 3, BOOT_A)), g0 + 1);
}

#[test]
fn boot_only_change_keeps_generation_but_updates_boot_id() {
    let rig = Rig::new();
    let g0 = gen_of(&report(&rig, T0, BOOT_A));
    let rb = report(&rig, T0 + 1, BOOT_B);
    assert_eq!(gen_of(&rb), g0);
    assert!(rb.contains(&format!("boot_id={BOOT_B}\n")));
    let st = HelperState::parse(&fs::read(rig.state_path()).unwrap()).unwrap();
    assert_eq!(st.boot_id, BOOT_B, "boot_id is persisted without a generation increment");
}

#[test]
fn generation_never_decreases_under_same_key() {
    let mut rig = Rig::new();
    let mut last = gen_of(&report(&rig, T0, BOOT_A));
    for (i, m) in [11u32, 11, 12, 13, 13].into_iter().enumerate() {
        bump(&mut rig, m);
        let g = gen_of(&report(&rig, T0 + 1 + u64::try_from(i).unwrap(), BOOT_A));
        assert!(g >= last && g <= last + 1);
        last = g;
    }
    assert_eq!(last, 4, "initial 1, then 11, 12, 13 are three distinct contents");
}

#[test]
fn hub_rejects_generation_regression() {
    let mut rig = Rig::new();
    let mut verify_gen = |rig: &mut Rig, t: u64| {
        let a = rig.attest_wire(t, BOOT_A);
        rig.hub.verify(HOST, &a, t)
    };
    assert!(matches!(verify_gen(&mut rig, T0), Ok(Outcome::Verified(_))));
    let old = fs::read(rig.state_path()).unwrap(); // generation 1
    for (i, m) in [21u32, 22, 23, 24].into_iter().enumerate() {
        bump(&mut rig, m);
        assert!(matches!(verify_gen(&mut rig, T0 + 1 + u64::try_from(i).unwrap()), Ok(Outcome::Verified(_))));
    }
    assert_eq!(rig.hub.highest_generation(&rig.pk), Some(5));
    // Restore the old, valid, correctly checksummed state file.
    fs::write(rig.state_path(), &old).unwrap();
    bump(&mut rig, 25);
    let r = verify_gen(&mut rig, T0 + 10);
    println!("REGRESSION restored generation-1 file, helper reported next generation: {r:?}");
    assert!(matches!(r, Err(Rejection::GenerationRegressed { highest: 5, .. })));
    assert!(rig.hub.gate(HOST, T0 + 10).is_ok(), "finding: the previous verified state is not revoked by a rejected report");
}

#[test]
fn restored_state_can_repeat_a_generation_with_different_content_and_the_hub_accepts_it() {
    // Finding: "never repeats" is not detectable. A restored file one step behind makes the
    // helper issue the highest generation again for different content; the hub rule only
    // rejects strictly lower generations.
    let mut rig = Rig::new();
    let mut v = |rig: &mut Rig, t: u64| {
        let a = rig.attest_wire(t, BOOT_A);
        rig.hub.verify(HOST, &a, t)
    };
    assert!(matches!(v(&mut rig, T0), Ok(Outcome::Verified(_))));
    bump(&mut rig, 31);
    assert!(matches!(v(&mut rig, T0 + 1), Ok(Outcome::Verified(_))));
    let behind = fs::read(rig.state_path()).unwrap(); // generation 2
    bump(&mut rig, 32);
    let Ok(Outcome::Verified(a)) = v(&mut rig, T0 + 2) else { panic!() };
    assert_eq!(a.identity.generation, 3);
    fs::write(rig.state_path(), &behind).unwrap();
    bump(&mut rig, 33);
    let Ok(Outcome::Verified(b)) = v(&mut rig, T0 + 3) else { panic!("accepted") };
    assert_eq!(b.identity.generation, 3);
    assert_ne!(a.identity.route_state_hash, b.identity.route_state_hash, "same generation, different content");
}

#[test]
fn state_file_has_the_exact_format_checksum_and_permissions() {
    let rig = Rig::new();
    let bytes = fs::read(rig.state_path()).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    let keys: Vec<&str> = lines.iter().map(|l| l.split_once('=').unwrap().0).collect();
    assert_eq!(keys, ["format", "key_fingerprint", "generation", "config_hash", "route_state_hash", "boot_id", "checksum"]);
    assert_eq!(lines[0], "format=1");
    assert_eq!(lines[2], "generation=1");
    assert!(text.ends_with('\n'));
    let split = text.rfind("checksum=").unwrap();
    assert_eq!(&text[split + 9..text.len() - 1], hex::encode(sha256(&bytes[..split])), "checksum covers all preceding bytes");
    assert_eq!(HelperState::parse(&bytes).unwrap().render(), bytes);
    assert_eq!((mode(&rig.dir.0), mode(&rig.state_path())), (0o700, 0o600));
    assert_eq!(mode(&rig.dir.0.join("attest.key")), 0o600);
    assert_eq!(fs::read(rig.dir.0.join("attest.key")).unwrap().len(), 32, "raw 32-byte seed (format unspecified)");
    let _ = report(&rig, T0, BOOT_A);
    assert_eq!((mode(&rig.dir.0.join("state.lock")), fs::metadata(rig.dir.0.join("state.lock")).unwrap().len()), (0o600, 0));
    assert!(!rig.dir.0.join("state.tmp").exists());
}

fn reason(rig: &Rig) -> String {
    match rig.attest_raw([4; 32], T0, BOOT_A) {
        Ok(a) => String::from_utf8(a.report).unwrap().lines().find_map(|l| l.strip_prefix("incomplete_reason=")).unwrap().to_owned(),
        Err(r10::helper::Unsigned(f)) => format!("unsigned:{}", f.reason()),
    }
}

fn apply_reason(rig: &Rig) -> Refused {
    let src = FixtureSource(rig.snap.clone());
    let e = env(T0, BOOT_A);
    let inp = Inputs { config_text: Some(&rig.toml), routes: &src, env: &e };
    let req = ContainRequest { target: "203.0.113.0/24".into(), ttl_s: 60, expected: None, agent_max_ttl: None, agent_max_active: None, agent_reported_active: None };
    apply_contain(&rig.dir, &req, &inp, &mut HelperRuntime { per_hour: 5, ..Default::default() }, &mut |_| {}, &mut |_| {}).unwrap_err()
}

#[test]
fn generation_state_missing_with_key_fails_closed() {
    let rig = Rig::new();
    fs::remove_file(rig.state_path()).unwrap();
    assert_eq!(reason(&rig), "state_missing");
    assert_eq!(apply_reason(&rig), Refused::SafetyUnknown("state_missing".into()));
}

#[test]
fn generation_state_corrupt_with_key_fails_closed() {
    let rig = Rig::new();
    let good = fs::read(rig.state_path()).unwrap();
    let text = String::from_utf8(good.clone()).unwrap();
    let recheck = |body: &str| {
        let sum = hex::encode(sha256(body.as_bytes()));
        format!("{body}checksum={sum}\n").into_bytes()
    };
    let body = &text[..text.rfind("checksum=").unwrap()];
    let variants: Vec<(&str, Vec<u8>)> = vec![
        ("truncated", good[..good.len() / 2].to_vec()),
        ("bad_checksum", text.replacen("generation=1", "generation=2", 1).into_bytes()),
        ("unknown_format", recheck(&body.replacen("format=1", "format=2", 1))),
        ("missing_field", recheck(&body.replacen("boot_id=aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa\n", "", 1))),
        ("empty_file", Vec::new()),
        ("leading_zero_generation", recheck(&body.replacen("generation=1", "generation=01", 1))),
    ];
    for (name, bytes) in variants {
        fs::write(rig.state_path(), bytes).unwrap();
        assert_eq!(reason(&rig), "state_corrupt", "{name}");
        assert_eq!(apply_reason(&rig), Refused::SafetyUnknown("state_corrupt".into()), "{name}");
        assert!(fs::read(rig.state_path()).is_ok(), "{name}: not deleted or reset");
    }
}

#[test]
fn generation_state_key_mismatch_fails_closed() {
    let rig = Rig::new();
    let mut st = HelperState::parse(&fs::read(rig.state_path()).unwrap()).unwrap();
    st.key_fingerprint = "ab".repeat(32);
    fs::write(rig.state_path(), st.render()).unwrap();
    assert_eq!(reason(&rig), "state_key_mismatch");
    assert_eq!(apply_reason(&rig), Refused::SafetyUnknown("state_key_mismatch".into()));
}

#[test]
fn key_missing_and_uninitialized_cannot_be_signed() {
    let rig = Rig::new();
    fs::remove_file(rig.dir.0.join("attest.key")).unwrap();
    assert_eq!(reason(&rig), "unsigned:key_missing");
    assert_eq!(apply_reason(&rig), Refused::SafetyUnknown("key_missing".into()));
    fs::remove_file(rig.state_path()).unwrap();
    assert_eq!(reason(&rig), "unsigned:uninitialized");
    let fresh = r10::state::HelperDir(rig.tmp.path().join("never-created"));
    assert!(matches!(fresh.begin().unwrap().failure(), Some(r10::state::FailClosed::Uninitialized)));
    assert!(!fresh.0.exists(), "reading an uninitialized helper creates nothing");
    let mut rig2 = Rig::new();
    fs::write(rig2.dir.0.join("attest.key"), [1u8; 5]).unwrap();
    assert_eq!(reason(&rig2), "unsigned:key_corrupt", "not in the contract's list");
    rig2.toml.clear();
}

#[test]
fn leftover_state_tmp_is_deleted_under_the_lock_at_start() {
    let rig = Rig::new();
    let g0 = gen_of(&report(&rig, T0, BOOT_A));
    fs::write(rig.dir.0.join("state.tmp"), b"half a state fi").unwrap();
    assert_eq!(gen_of(&report(&rig, T0 + 1, BOOT_A)), g0);
    assert!(!rig.dir.0.join("state.tmp").exists());
}

#[test]
fn interrupted_state_write_never_reuses_generation() {
    let mut rig = Rig::new();
    let g0 = gen_of(&report(&rig, T0, BOOT_A));
    let before = fs::read(rig.state_path()).unwrap();
    bump(&mut rig, 41);
    // The directory becomes unwritable: state.tmp cannot be created.
    fs::set_permissions(&rig.dir.0, fs::Permissions::from_mode(0o500)).unwrap();
    let r = report(&rig, T0 + 1, BOOT_A);
    fs::set_permissions(&rig.dir.0, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(r.contains("complete=false\nincomplete_reason=state_write_failed\n"), "{r}");
    assert_eq!(gen_of(&r), g0, "generation not advanced in the report");
    assert_eq!(fs::read(rig.state_path()).unwrap(), before, "old file intact");
    // Retried on the next request: exactly one increment, issued once.
    assert_eq!(gen_of(&report(&rig, T0 + 2, BOOT_A)), g0 + 1);
    assert_eq!(gen_of(&report(&rig, T0 + 3, BOOT_A)), g0 + 1);
}

#[test]
fn write_hook_sees_every_stage_in_order() {
    let mut rig = Rig::new();
    bump(&mut rig, 51);
    let src = FixtureSource(rig.snap.clone());
    let e = env(T0, BOOT_A);
    let mut seen = Vec::new();
    r10::helper::attest(&rig.dir, [4; 32], &Inputs { config_text: Some(&rig.toml), routes: &src, env: &e }, &mut |s| seen.push(s)).unwrap();
    use r10::state::Stage::*;
    assert_eq!(seen, [TmpWritten, TmpSynced, Renamed, DirSynced]);
}
