//! What the strict report parser refuses, one named case at a time.

use domovoi_core::attest::{
    EncodeError, MAX_REPORT_BYTES, Report, ReportError, check_line, parse_report, render_complete,
    render_incomplete,
};

use crate::common::{complete_report, incomplete_report, vector_text};

fn valid() -> Vec<u8> {
    render_complete(&complete_report())
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("utf-8")
}

fn expect_err(name: &str, bytes: &[u8], check: impl Fn(&ReportError) -> bool) {
    let err = parse_report(bytes).expect_err(name);
    assert!(check(&err), "{name}: unexpected error {err:?}");
}

#[test]
fn valid_reports_parse_and_render_back_byte_for_byte() {
    let bytes = valid();
    let Ok(Report::Complete(report)) = parse_report(&bytes) else {
        panic!("a complete report");
    };
    assert_eq!(*report, complete_report());
    assert_eq!(render_complete(&report), bytes);
    report.check_hashes().expect("hashes recompute");
    assert!(text(&bytes).contains("complete=true\nincomplete_reason=\n"));

    let incomplete = render_incomplete(&incomplete_report());
    let Ok(Report::Incomplete(parsed)) = parse_report(&incomplete) else {
        panic!("an incomplete report");
    };
    assert_eq!(parsed, incomplete_report());
    assert_eq!(render_incomplete(&parsed), incomplete);
}

#[test]
fn report_rejects_invalid_encoding_vectors() {
    let mut rows = 0;
    for row in vector_text("invalid_lines.txt")
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
    {
        let (name, line) = row.split_once('\t').expect("a row");
        let err = check_line(line).expect_err(name);
        let ok = match name {
            "raw_pipe_extra_field" => matches!(
                &err,
                ReportError::Value {
                    err: EncodeError::FieldCount {
                        expected: 2,
                        found: 3
                    },
                    ..
                }
            ),
            "lowercase_hex" => matches!(
                &err,
                ReportError::Value {
                    err: EncodeError::LowercaseHex(_),
                    ..
                }
            ),
            "encoded_literal" => matches!(
                &err,
                ReportError::Value {
                    err: EncodeError::EncodedLiteral(_),
                    ..
                }
            ),
            "cidr_host_bits" | "ipv6_not_rfc5952" => matches!(
                &err,
                ReportError::Value {
                    err: EncodeError::NotCanonical { .. },
                    ..
                }
            ),
            _ => false,
        };
        assert!(ok, "{name}: {err:?}");
        rows += 1;
    }
    assert_eq!(rows, 5);
}

#[test]
fn report_parser_rejects_duplicates_unsorted_and_noncanonical() {
    let s = text(&valid());
    let routes: Vec<&str> = s.lines().filter(|l| l.starts_with("route=")).collect();
    assert!(routes.len() >= 2);
    let duplicated = s.replacen(routes[0], &format!("{}\n{}", routes[0], routes[0]), 1);
    expect_err("duplicate route line", duplicated.as_bytes(), |e| {
        matches!(e, ReportError::DuplicateLine(_))
    });
    let swapped = s
        .replacen(routes[0], "@@A", 1)
        .replacen(routes[1], routes[0], 1)
        .replacen("@@A", routes[1], 1);
    expect_err("unsorted route lines", swapped.as_bytes(), |e| {
        matches!(e, ReportError::UnsortedLine(_))
    });
    let leading_zero = s.replacen("hub_addr=192.0.2.10", "hub_addr=192.0.2.010", 1);
    expect_err("hub_addr leading zero", leading_zero.as_bytes(), |e| {
        matches!(e, ReportError::Value { .. })
    });
    expect_err(
        "generation leading zero",
        s.replacen("generation=1", "generation=01", 1).as_bytes(),
        |e| matches!(e, ReportError::Value { .. }),
    );
}

#[test]
fn parser_is_strict_on_boundaries_the_vectors_do_not_list() {
    let bytes = valid();
    let s = text(&bytes);
    let mut no_lf = bytes.clone();
    no_lf.pop();
    expect_err("missing final LF", &no_lf, |e| {
        matches!(e, ReportError::MissingFinalLf)
    });
    expect_err("CR", s.replacen('\n', "\r\n", 1).as_bytes(), |e| {
        matches!(e, ReportError::Cr)
    });
    let mut bom = vec![0xEF, 0xBB, 0xBF];
    bom.extend_from_slice(&bytes);
    expect_err("BOM", &bom, |e| matches!(e, ReportError::Bom));
    expect_err("blank line", s.replacen('\n', "\n\n", 1).as_bytes(), |e| {
        matches!(e, ReportError::BlankLine(_))
    });
    expect_err(
        "trailing space",
        s.replacen("format=1\n", "format=1 \n", 1).as_bytes(),
        |e| matches!(e, ReportError::TrailingWhitespace(_)),
    );
    expect_err("unknown key", format!("{s}extra=1\n").as_bytes(), |e| {
        matches!(e, ReportError::UnknownKey(_))
    });
    let max_active = s
        .lines()
        .find(|l| l.starts_with("max_active="))
        .expect("line");
    expect_err(
        "missing key",
        s.replacen(&format!("{max_active}\n"), "", 1).as_bytes(),
        |e| matches!(e, ReportError::MissingKey("max_active")),
    );
    let (nonce, boot) = (
        s.lines().nth(2).expect("nonce"),
        s.lines().nth(3).expect("boot"),
    );
    let out_of_order = s.replacen(
        &format!("{nonce}\n{boot}\n"),
        &format!("{boot}\n{nonce}\n"),
        1,
    );
    expect_err("out of order key", out_of_order.as_bytes(), |e| {
        matches!(e, ReportError::OutOfOrder(_))
    });
    expect_err(
        "duplicate single key",
        s.replacen("format=1\n", "format=1\nformat=1\n", 1)
            .as_bytes(),
        |e| matches!(e, ReportError::DuplicateKey(_)),
    );
    let route = s.lines().find(|l| l.starts_with("route=")).expect("route");
    expect_err(
        "wrong field count",
        s.replacen(route, &format!("{route}|9"), 1).as_bytes(),
        |e| {
            matches!(
                e,
                ReportError::Value {
                    err: EncodeError::FieldCount { .. },
                    ..
                }
            )
        },
    );
    expect_err(
        "truncated escape",
        s.replacen("|eth0\n", "|eth%4\n", 1).as_bytes(),
        |e| {
            matches!(
                e,
                ReportError::Value {
                    err: EncodeError::TruncatedEscape,
                    ..
                }
            )
        },
    );
    expect_err(
        "raw non-ASCII",
        s.replacen("|eth0\n", "|\u{e9}th0\n", 1).as_bytes(),
        |e| {
            matches!(
                e,
                ReportError::Value {
                    err: EncodeError::RawByte(0xC3),
                    ..
                }
            )
        },
    );
    expect_err(
        "format 2",
        s.replacen("format=1", "format=2", 1).as_bytes(),
        |e| matches!(e, ReportError::BadFormat(_)),
    );
    expect_err("empty input", b"", |e| {
        matches!(e, ReportError::MissingFinalLf)
    });
}

#[test]
fn header_values_must_be_in_their_grammar() {
    let s = text(&valid());
    for (from, to) in [
        (
            "helper_key_id=0123456789abcdef",
            "helper_key_id=0123456789ABCDEF",
        ),
        (
            "helper_key_id=0123456789abcdef",
            "helper_key_id=0123456789abcde",
        ),
        (
            "boot_id=5f0c1d2e-3a4b-4c5d-8e9f-0a1b2c3d4e5f",
            "boot_id=5F0C1D2E-3A4B-4C5D-8E9F-0A1B2C3D4E5F",
        ),
        (
            "boot_id=5f0c1d2e-3a4b-4c5d-8e9f-0a1b2c3d4e5f",
            "boot_id=5f0c1d2e3a4b4c5d8e9f0a1b2c3d4e5f",
        ),
        ("complete=true", "complete=yes"),
        (
            "computed_at=2023-11-14T22:13:20Z",
            "computed_at=2023-11-14T22:13:20.5Z",
        ),
        (
            "computed_at=2023-11-14T22:13:20Z",
            "computed_at=2023-11-14T22:13:20+00:00",
        ),
        (
            "computed_at=2023-11-14T22:13:20Z",
            "computed_at=2023-02-30T22:13:20Z",
        ),
        (
            "computed_at=2023-11-14T22:13:20Z",
            "computed_at=2023-11-14T22:13:60Z",
        ),
        (
            "computed_at=2023-11-14T22:13:20Z",
            "computed_at=2023-11-14 22:13:20Z",
        ),
    ] {
        assert!(s.contains(from), "{from}");
        let mutated = s.replacen(from, to, 1);
        assert!(parse_report(mutated.as_bytes()).is_err(), "{to}");
    }
}

#[test]
fn no_other_key_may_carry_an_empty_value() {
    let s = text(&valid());
    let keys: Vec<String> = s
        .lines()
        .map(|l| l.split_once('=').expect("a key").0.to_owned())
        .collect();
    let mut rejected = 0;
    for key in keys.iter().filter(|k| *k != "incomplete_reason") {
        let line = s
            .lines()
            .find(|l| l.starts_with(&format!("{key}=")))
            .expect("line");
        let blank = s.replacen(line, &format!("{key}="), 1);
        let err = parse_report(blank.as_bytes()).expect_err(key);
        assert!(matches!(err, ReportError::EmptyValue(_)), "{key}: {err:?}");
        rejected += 1;
    }
    assert!(rejected >= 15);
}

#[test]
fn incomplete_reason_empty_only_when_complete() {
    let ok = valid();
    let s = text(&ok);
    let lie = s.replacen("complete=true", "complete=false", 1);
    expect_err("complete=false without a reason", lie.as_bytes(), |e| {
        matches!(e, ReportError::ReasonInconsistent(_))
    });
    let both = s.replacen("incomplete_reason=", "incomplete_reason=x", 1);
    expect_err("complete=true with a reason", both.as_bytes(), |e| {
        matches!(e, ReportError::ReasonInconsistent(_))
    });
    let incomplete = text(&render_incomplete(&incomplete_report()));
    let emptied = incomplete.replacen("incomplete_reason=state_missing", "incomplete_reason=", 1);
    expect_err("incomplete with an empty reason", emptied.as_bytes(), |e| {
        matches!(e, ReportError::ReasonInconsistent(_))
    });
    for token in ["State_missing", "1abc", "has space", "x=y", &"a".repeat(65)] {
        let bad = incomplete.replacen("state_missing", token, 1);
        assert!(parse_report(bad.as_bytes()).is_err(), "{token}");
    }
    assert!(
        parse_report(
            incomplete
                .replacen("state_missing", &"a".repeat(64), 1)
                .as_bytes()
        )
        .is_ok()
    );
    assert!(
        parse_report(
            incomplete
                .replacen("state_missing", "an_unknown_token_9", 1)
                .as_bytes()
        )
        .is_ok()
    );
}

#[test]
fn incomplete_report_has_minimal_shape() {
    let incomplete = text(&render_incomplete(&incomplete_report()));
    assert_eq!(
        incomplete,
        "format=1\nhelper_key_id=0123456789abcdef\nnonce=0909090909090909090909090909090909090909090909090909090909090909\nboot_id=5f0c1d2e-3a4b-4c5d-8e9f-0a1b2c3d4e5f\ngeneration=0\nconfig_hash=0000000000000000000000000000000000000000000000000000000000000000\nroute_state_hash=0000000000000000000000000000000000000000000000000000000000000000\ncomplete=false\nincomplete_reason=state_missing\ncomputed_at=2023-11-14T22:13:20Z\nmax_ttl_s=0\nmax_active=0\n"
    );
    for (from, to) in [
        ("generation=0", "generation=1"),
        ("max_ttl_s=0", "max_ttl_s=3600"),
        ("max_active=0", "max_active=1"),
        ("config_hash=00", "config_hash=01"),
        ("route_state_hash=00", "route_state_hash=01"),
    ] {
        let bad = incomplete.replacen(from, to, 1);
        expect_err(to, bad.as_bytes(), |e| {
            matches!(e, ReportError::IncompleteShape(_))
        });
    }
}

#[test]
fn incomplete_report_with_repeated_lines_rejected() {
    let incomplete = text(&render_incomplete(&incomplete_report()));
    for extra in ["cidr=192.0.2.0/24\n", "hub_addr=192.0.2.10\n"] {
        let (head, tail) = incomplete.split_at(incomplete.find("max_ttl_s").expect("config"));
        let with_cidr = if extra.starts_with("cidr") {
            format!("{head}{extra}{tail}")
        } else {
            format!("{incomplete}{extra}")
        };
        expect_err(extra, with_cidr.as_bytes(), |e| {
            matches!(e, ReportError::IncompleteShape(_))
        });
    }
}

#[test]
fn complete_report_with_zero_hash_rejected() {
    let s = text(&valid());
    let config = s
        .lines()
        .find(|l| l.starts_with("config_hash="))
        .expect("line");
    let routes = s
        .lines()
        .find(|l| l.starts_with("route_state_hash="))
        .expect("line");
    let zero = "0".repeat(64);
    expect_err(
        "zero config hash",
        s.replacen(config, &format!("config_hash={zero}"), 1)
            .as_bytes(),
        |e| matches!(e, ReportError::ZeroHash("config_hash")),
    );
    expect_err(
        "zero route hash",
        s.replacen(routes, &format!("route_state_hash={zero}"), 1)
            .as_bytes(),
        |e| matches!(e, ReportError::ZeroHash("route_state_hash")),
    );
}

#[test]
fn complete_report_with_generation_zero_rejected() {
    let s = text(&valid());
    expect_err(
        "generation 0",
        s.replacen("generation=1", "generation=0", 1).as_bytes(),
        |e| matches!(e, ReportError::GenerationZero),
    );
}

#[test]
fn hash_mismatch_is_found_by_recomputation_not_by_the_parser() {
    let s = text(&valid());
    let config = s
        .lines()
        .find(|l| l.starts_with("config_hash="))
        .expect("line");
    let forged = s.replacen(config, &format!("config_hash={}", "ab".repeat(32)), 1);
    let Ok(Report::Complete(report)) = parse_report(forged.as_bytes()) else {
        panic!("grammar-valid");
    };
    assert_eq!(
        report.check_hashes(),
        Err(ReportError::HashMismatch("config_hash"))
    );
}

#[test]
fn invalid_utf8_is_rejected_as_its_own_step() {
    let s = valid();
    for (name, bad) in [
        ("lone 0xff", vec![0xFF_u8]),
        ("overlong c0 80", vec![0xC0, 0x80]),
    ] {
        let mut bytes = s.clone();
        let at = bytes.iter().position(|&b| b == b'x').unwrap_or(10);
        bytes.splice(at..=at, bad);
        expect_err(name, &bytes, |e| matches!(e, ReportError::NotUtf8(_)));
    }
}

#[test]
fn oversize_report_is_refused() {
    let mut bytes = valid();
    bytes.resize(MAX_REPORT_BYTES + 1, b'\n');
    expect_err(
        "oversize",
        &bytes,
        |e| matches!(e, ReportError::TooLarge(n) if *n == MAX_REPORT_BYTES + 1),
    );
}

#[test]
fn report_lines_are_validated_by_key() {
    for line in [
        "rule=inet|0|all|255|x",
        "rule=inet|0|all|-1",
        "rule=inet|0|NOT|255",
        "route=inet|254|0.0.0.0/0|-|eth0",
        "protected_unit=svc-a",
        "protected_unit=svc%5Ca.service",
        "protected_container=-bad",
        "link_peer=192.0.2.1|0",
        "link_peer=192.0.2.1|65536",
        "local_addr=192.0.2.20|eth0",
        "endpoint_route=192.0.2.10|254|-|eth0",
        "hub_addr=",
        "format=1",
    ] {
        assert!(check_line(line).is_err(), "{line}");
    }
    for line in [
        "protected_unit=svc-a.service",
        "protected_container=ctr-a",
        "link_peer=192.0.2.1|443",
        "local_addr=192.0.2.20/24|eth0",
        "cidr=2001:db8::/32",
    ] {
        check_line(line).unwrap_or_else(|err| panic!("{line}: {err}"));
    }
}
