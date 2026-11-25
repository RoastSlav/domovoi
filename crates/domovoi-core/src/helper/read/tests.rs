//! Tests for the read slots, bounds, tokens and wire shapes.

use uuid::Uuid;

use super::*;
use crate::helper::{
    HELPER_PROTOCOL, HelperGroup, HelperRequest, HelperRequestFrame, HelperResponse,
    HelperResponseFrame, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, decode_request, encode_frame,
    frame_len,
};
use crate::identity::RefusalToken;

fn frame_of(request: &str) -> Vec<u8> {
    format!(
        r#"{{"v":1,"id":"{}","request":{request}}}"#,
        Uuid::from_u128(1)
    )
    .into_bytes()
}

#[test]
fn container_ref_rejects_path_and_option_characters() {
    for bad in [
        "",
        "-rf",
        ".hidden",
        "_x",
        "a/b",
        "../x",
        "a?b",
        "a#b",
        "a%2f",
        "a b",
        "a\n",
        "a;b",
        "é",
        &"a".repeat(129),
    ] {
        assert_eq!(
            ContainerRef::parse(bad),
            Err(SlotError::Container),
            "{bad:?}"
        );
    }
}

#[test]
fn container_ref_accepts_docker_names_and_ids() {
    for good in [
        "web",
        "web-1",
        "my_app.v2",
        "0a1b2c",
        &"a".repeat(128),
        &"f".repeat(64),
    ] {
        assert_eq!(ContainerRef::parse(good).expect("valid").as_str(), good);
    }
}

#[test]
fn device_ref_rejects_anything_but_dev_alnum() {
    for bad in [
        "",
        "/dev/",
        "sda",
        "/dev/sda1/../x",
        "/dev/SDA",
        "/dev/sd a",
        "/dev/-x",
        "/dev/sd-a",
        "/dev//sda",
        "/etc/passwd",
        "/dev/nvme0n1p1/",
        &format!("/dev/{}", "a".repeat(33)),
    ] {
        assert_eq!(DeviceRef::parse(bad), Err(SlotError::Device), "{bad:?}");
    }
    for good in [
        "/dev/sda",
        "/dev/nvme0n1",
        "/dev/sg0",
        &format!("/dev/{}", "a".repeat(32)),
    ] {
        assert!(DeviceRef::parse(good).is_ok(), "{good}");
    }
}

#[test]
fn a_bad_slot_is_a_malformed_frame() {
    let body = frame_of(r#"{"op":"docker_inspect","container":"a/b"}"#);
    assert_eq!(decode_request(&body), Err(RefusalToken::FrameMalformed));
    let body = frame_of(r#"{"op":"smart_report","device":"/etc/passwd"}"#);
    assert_eq!(decode_request(&body), Err(RefusalToken::FrameMalformed));
}

#[test]
fn read_request_bounds_are_enforced_in_decode() {
    let bad = [
        r#"{"op":"docker_logs","container":"web","lines":0}"#,
        r#"{"op":"docker_logs","container":"web","lines":501}"#,
        r#"{"op":"kernel_log","since_minutes":0,"lines":10}"#,
        r#"{"op":"kernel_log","since_minutes":1441,"lines":10}"#,
        r#"{"op":"kernel_log","since_minutes":10,"lines":0}"#,
        r#"{"op":"kernel_log","since_minutes":10,"lines":501}"#,
        r#"{"op":"docker_events","since_secs":86401,"until_secs":0}"#,
        r#"{"op":"docker_events","since_secs":10,"until_secs":10}"#,
        r#"{"op":"docker_events","since_secs":5,"until_secs":10}"#,
        r#"{"op":"docker_logs","container":"web","lines":10,"path":"/etc"}"#,
        r#"{"op":"docker_list","extra":1}"#,
    ];
    for request in bad {
        assert_eq!(
            decode_request(&frame_of(request)),
            Err(RefusalToken::FrameMalformed),
            "{request}"
        );
    }
    let good = [
        r#"{"op":"docker_logs","container":"web","lines":1}"#,
        r#"{"op":"docker_logs","container":"web","lines":500}"#,
        r#"{"op":"kernel_log","since_minutes":1440,"lines":500}"#,
        r#"{"op":"docker_events","since_secs":86400,"until_secs":0}"#,
        r#"{"op":"docker_events","since_secs":2,"until_secs":1}"#,
    ];
    for request in good {
        assert!(decode_request(&frame_of(request)).is_ok(), "{request}");
    }
}

#[test]
fn helper_read_requests_round_trip_every_variant() {
    let requests = [
        HelperRequest::DockerProbe,
        HelperRequest::DockerList,
        HelperRequest::DockerEvents {
            since_secs: 600,
            until_secs: 60,
        },
        HelperRequest::DockerInspect {
            container: ContainerRef::parse("web-1").expect("valid"),
        },
        HelperRequest::DockerLogs {
            container: ContainerRef::parse("web-1").expect("valid"),
            lines: 200,
        },
        HelperRequest::SmartScan,
        HelperRequest::SmartReport {
            device: DeviceRef::parse("/dev/sda").expect("valid"),
        },
        HelperRequest::KernelLog {
            since_minutes: 60,
            lines: 100,
        },
        HelperRequest::NetListeners,
    ];
    let mut wire = Vec::new();
    for request in requests {
        let frame = HelperRequestFrame {
            v: HELPER_PROTOCOL,
            id: Uuid::from_u128(1),
            request,
        };
        let bytes = encode_frame(&frame).expect("encodes");
        assert_eq!(decode_request(&bytes[4..]), Ok(frame.clone()));
        wire.push(serde_json::to_value(&frame.request).expect("serialises"));
    }
    insta::assert_json_snapshot!(wire);
}

#[test]
fn read_op_group_mapping_is_exact() {
    let ask = |op: &str| {
        let body = match op {
            "docker_events" => r#"{"op":"docker_events","since_secs":2,"until_secs":1}"#.to_owned(),
            "docker_inspect" => r#"{"op":"docker_inspect","container":"a"}"#.to_owned(),
            "docker_logs" => r#"{"op":"docker_logs","container":"a","lines":1}"#.to_owned(),
            "smart_report" => r#"{"op":"smart_report","device":"/dev/sda"}"#.to_owned(),
            "kernel_log" => r#"{"op":"kernel_log","since_minutes":1,"lines":1}"#.to_owned(),
            other => format!(r#"{{"op":"{other}"}}"#),
        };
        decode_request(&frame_of(&body))
            .expect("decodes")
            .request
            .group()
    };
    for op in [
        "docker_probe",
        "docker_list",
        "docker_events",
        "docker_inspect",
        "docker_logs",
    ] {
        assert_eq!(ask(op), Some(HelperGroup::DockerRead), "{op}");
    }
    for op in ["smart_scan", "smart_report"] {
        assert_eq!(ask(op), Some(HelperGroup::SmartRead), "{op}");
    }
    assert_eq!(ask("kernel_log"), Some(HelperGroup::KernelRead));
    assert_eq!(ask("net_listeners"), Some(HelperGroup::NetRead));
    assert_eq!(ask("health"), None);
    assert_eq!(ask("groups"), None);
    assert_eq!(ask("safety_public_key"), Some(HelperGroup::SafetyAttest));
}

#[test]
fn read_tokens_have_stable_spellings() {
    let spellings: Vec<&str> = ReadToken::ALL.iter().map(|t| t.as_str()).collect();
    insta::assert_json_snapshot!(spellings);
    for token in ReadToken::ALL {
        let json = serde_json::to_string(token).expect("serialises");
        assert_eq!(json, format!("\"{}\"", token.as_str()));
        assert_eq!(
            serde_json::from_str::<ReadToken>(&json).expect("parses"),
            *token
        );
    }
}

#[test]
fn request_and_response_frame_caps_are_split() {
    assert_eq!(MAX_REQUEST_BYTES, 65_536);
    assert_eq!(MAX_RESPONSE_BYTES, 4_194_304);
}

#[test]
fn frame_len_takes_the_limit() {
    let header = 1_000_u32.to_be_bytes();
    assert_eq!(frame_len(header, 1_000), Ok(1_000));
    assert!(frame_len(header, 999).is_err());
}

#[test]
fn read_failed_detail_is_bounded_ascii_and_has_no_paths() {
    let long = "x".repeat(500);
    let cleaned = ReadFailure::clean_detail(&long).expect("some");
    assert_eq!(cleaned.len(), DETAIL_MAX_BYTES);
    let dirty = ReadFailure::clean_detail(
        "cannot open /var/lib/secret/key: Permission denied\nsecond line",
    )
    .expect("some");
    assert_eq!(dirty, "cannot open ? Permission denied");
    assert_eq!(
        ReadFailure::clean_detail("failed /dev/sda: no \u{e9}").as_deref(),
        Some("failed /dev/sda: no ?")
    );
    assert_eq!(ReadFailure::clean_detail("  \n"), None);
}

#[test]
fn read_answers_round_trip() {
    let answer = HelperResponse::ReadFailed {
        group: HelperGroup::SmartRead,
        token: ReadToken::ToolMissing,
        detail: None,
    };
    let frame = HelperResponseFrame {
        v: HELPER_PROTOCOL,
        id: Uuid::from_u128(1),
        response: answer,
    };
    let bytes = encode_frame(&frame).expect("encodes");
    assert_eq!(
        crate::helper::decode_response(&bytes[4..]).expect("decodes"),
        frame
    );
}
