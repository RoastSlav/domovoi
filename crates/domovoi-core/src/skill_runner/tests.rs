//! Tests for the runner protocol types and framing.

use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;
use uuid::Uuid;

use super::*;
use crate::action::ImplHash;
use crate::id::ExecutionId;
use crate::identity::HostDirs;
use crate::isolation::{IsolationReport, ProcStatus};
use crate::package::{PackageName, PackageVersion};
use crate::sha256::ScriptDigest;
use crate::skill::SkillName;

fn run_request(source: SkillSource) -> RunRequest {
    let manifest = crate::skill::ManifestHash::digest(b"manifest");
    let entry = ScriptDigest::digest(b"entry");
    RunRequest {
        protocol: RUNNER_PROTOCOL,
        execution_id: ExecutionId::from_uuid(Uuid::from_u128(
            0x0123_4567_89ab_cdef_0123_4567_89ab_cdef,
        )),
        source,
        skill: SkillName::parse("disk_usage").expect("valid name"),
        impl_hash: ImplHash::from_parts(manifest, entry),
        args: BTreeMap::from([("path".to_owned(), "/var".to_owned())]),
        timeout_secs: 30,
        max_output_bytes: 65_536,
    }
}

fn empty_report() -> IsolationReport {
    IsolationReport {
        dirs: HostDirs::default(),
        connects: vec![],
        hidden_state: vec![],
        etc_listing: None,
        var_lib_listing: None,
        config_files: vec![],
        run: None,
        writes: vec![],
        homes: vec![],
        run_user_exists: false,
        proc_hidden: vec![],
        status: ProcStatus {
            uid: 991,
            gid: 991,
            groups: vec![],
            cap_eff: 0,
            cap_bnd: 0,
            no_new_privs: true,
        },
        mounts: vec![],
        apparmor_label: None,
        shadow: None,
    }
}

fn run_reply() -> RunReply {
    RunReply {
        exit_code: Some(0),
        signal: None,
        stdout: "ok\n".into(),
        stderr: String::new(),
        truncated: false,
        timed_out: false,
        duration_ms: 12,
        argv_hash: "ab".repeat(32),
    }
}

fn every_request() -> Vec<RunnerRequest> {
    vec![
        RunnerRequest::Health(Header {
            protocol: RUNNER_PROTOCOL,
        }),
        RunnerRequest::Run(run_request(SkillSource::SkillsDir)),
        RunnerRequest::Run(run_request(SkillSource::Package {
            name: PackageName::parse("nginx-tools").expect("valid name"),
            version: PackageVersion::parse("1.2.3").expect("valid version"),
        })),
        RunnerRequest::Probe(ProbeRequest {
            protocol: RUNNER_PROTOCOL,
            helper_pid: Some(4242),
        }),
    ]
}

fn every_reply() -> Vec<RunnerReply> {
    vec![
        RunnerReply::Health(HealthReply {
            protocol: RUNNER_PROTOCOL,
            version: "0.1.0".into(),
            skills_root: "/etc/domovoi/skills".into(),
            packages_root: "/var/lib/domovoi-packages".into(),
        }),
        RunnerReply::Run(run_reply()),
        RunnerReply::Probe(Box::new(empty_report())),
        RunnerReply::Refused(Refusal {
            code: RefusalCode::RunAsUnsupported,
            detail: "the manifest names another user".into(),
        }),
    ]
}

fn split(frame: &[u8]) -> ([u8; 4], &[u8]) {
    let mut header = [0u8; 4];
    header.copy_from_slice(&frame[..4]);
    (header, &frame[4..])
}

#[test]
fn frame_round_trips_every_request_and_reply_kind() {
    for request in every_request() {
        let frame = encode_frame(&request, MAX_REQUEST_BYTES).expect("encode");
        let (header, body) = split(&frame);
        assert_eq!(frame_len(header, MAX_REQUEST_BYTES), Ok(body.len()));
        assert_eq!(decode_payload::<RunnerRequest>(body), Ok(request));
    }
    for reply in every_reply() {
        let frame = encode_frame(&reply, MAX_FRAME_BYTES).expect("encode");
        let (header, body) = split(&frame);
        assert_eq!(frame_len(header, MAX_FRAME_BYTES), Ok(body.len()));
        assert_eq!(decode_payload::<RunnerReply>(body), Ok(reply));
    }
}

#[test]
fn frame_longer_than_limit_is_refused_before_reading_the_body() {
    let header = u32::try_from(MAX_REQUEST_BYTES + 1)
        .expect("fits")
        .to_be_bytes();
    assert_eq!(
        frame_len(header, MAX_REQUEST_BYTES),
        Err(FrameError::TooLarge {
            len: MAX_REQUEST_BYTES + 1,
            limit: MAX_REQUEST_BYTES
        })
    );
    assert_eq!(frame_len([0; 4], MAX_REQUEST_BYTES), Err(FrameError::Empty));
    let big = RunnerReply::Run(RunReply {
        stdout: "x".repeat(2_000),
        ..run_reply()
    });
    assert!(matches!(
        encode_frame(&big, 1_000),
        Err(FrameError::TooLarge { .. })
    ));
}

#[test]
fn request_with_unknown_field_is_refused() {
    let mut value =
        serde_json::to_value(RunnerRequest::Health(Header { protocol: 1 })).expect("json");
    value["extra"] = serde_json::json!(true);
    let body = serde_json::to_vec(&value).expect("bytes");
    assert!(matches!(
        decode_payload::<RunnerRequest>(&body),
        Err(FrameError::Malformed(_))
    ));

    let mut run = serde_json::to_value(RunnerRequest::Run(run_request(SkillSource::SkillsDir)))
        .expect("json");
    run["env"] = serde_json::json!({ "PATH": "/tmp" });
    let body = serde_json::to_vec(&run).expect("bytes");
    assert!(matches!(
        decode_payload::<RunnerRequest>(&body),
        Err(FrameError::Malformed(_))
    ));
}

#[test]
fn run_request_has_no_path_env_or_user_field() {
    let value = serde_json::to_value(run_request(SkillSource::SkillsDir)).expect("json");
    let keys: BTreeSet<&str> = value
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    let expected: BTreeSet<&str> = [
        "protocol",
        "execution_id",
        "source",
        "skill",
        "impl_hash",
        "args",
        "timeout_secs",
        "max_output_bytes",
    ]
    .into_iter()
    .collect();
    assert_eq!(keys, expected);
}

#[test]
fn refusal_codes_are_unique_snake_case() {
    let mut seen = BTreeSet::new();
    for code in RefusalCode::ALL {
        let token = code.as_str();
        assert!(
            token.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "{token}"
        );
        assert!(seen.insert(token), "duplicate {token}");
        let json = serde_json::to_string(code).expect("json");
        assert_eq!(json, format!("\"{token}\""));
    }
    assert_eq!(seen.len(), 12);
}

#[test]
fn request_frame_snapshot() {
    let rendered: Vec<String> = every_request()
        .iter()
        .map(|r| String::from_utf8(serde_json::to_vec(r).expect("json")).expect("utf8"))
        .collect();
    insta::assert_snapshot!(rendered.join("\n"));
}

#[test]
fn reply_frame_snapshot() {
    let rendered: Vec<String> = every_reply()
        .iter()
        .filter(|r| !matches!(r, RunnerReply::Probe(_)))
        .map(|r| String::from_utf8(serde_json::to_vec(r).expect("json")).expect("utf8"))
        .collect();
    insta::assert_snapshot!(rendered.join("\n"));
}

#[test]
fn package_names_cannot_traverse() {
    assert!(PackageName::parse("../etc").is_err());
    assert!(PackageVersion::parse("1/2").is_err());
    assert!(SkillName::parse("a/b").is_err());
}

proptest! {
    #[test]
    fn decode_never_panics_on_arbitrary_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        let _ = decode_payload::<RunnerRequest>(&bytes);
        let _ = decode_payload::<RunnerReply>(&bytes);
        if bytes.len() >= 4 {
            let mut header = [0u8; 4];
            header.copy_from_slice(&bytes[..4]);
            let _ = frame_len(header, MAX_REQUEST_BYTES);
        }
    }
}
