//! A5-A7 and the `incomplete_reason` conflict: what the strict parser rejects.

mod common;

use common::{BOOT_A, Rig, T0};
use r10::encoding::EncodingError;
use r10::report::{EmptyReasonPolicy, ParseError, check_line, parse_report};

const DEFAULT: EmptyReasonPolicy = EmptyReasonPolicy::EmptyIffComplete;

fn valid() -> Vec<u8> {
    let rig = Rig::new();
    rig.attest_raw([9; 32], T0, BOOT_A).expect("signed").report
}

fn text(b: &[u8]) -> String {
    String::from_utf8(b.to_vec()).expect("utf8")
}

fn expect_err(name: &str, bytes: &[u8], check: impl Fn(&ParseError) -> bool) {
    let e = parse_report(bytes, DEFAULT).expect_err(name);
    println!("REJECT {name}: {e}");
    assert!(check(&e), "{name}: unexpected error {e:?}");
}

#[test]
fn valid_report_parses_and_round_trips_byte_for_byte() {
    let b = valid();
    let r = parse_report(&b, DEFAULT).expect("valid");
    assert!(r.complete && r.incomplete_reason.is_none());
    assert_eq!(r.render(), b, "render(parse(bytes)) == bytes");
    r.check_hashes().expect("hashes recompute");
    assert!(b.windows(19).any(|w| w == b"incomplete_reason=\n"), "complete report carries the empty value");
}

#[test]
fn report_rejects_invalid_encoding_vectors() {
    let file = std::fs::read_to_string(format!("{}/fixtures/invalid_lines.txt", env!("CARGO_MANIFEST_DIR"))).expect("fixture");
    let mut n = 0;
    for row in file.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        let (name, line) = row.split_once('\t').expect("row");
        let e = check_line(line).expect_err(name);
        println!("REJECT {name} `{line}`: {e}");
        let ok = match name {
            "raw_pipe_extra_field" => matches!(&e, ParseError::Value { err: EncodingError::FieldCount { expected: 2, found: 3 }, .. }),
            "lowercase_hex" => matches!(&e, ParseError::Value { err: EncodingError::LowercaseHex(_), .. }),
            "encoded_literal" => matches!(&e, ParseError::Value { err: EncodingError::EncodedLiteral(_), .. }),
            "cidr_host_bits" | "ipv6_not_rfc5952" => matches!(&e, ParseError::Value { err: EncodingError::NotCanonical { .. }, .. }),
            _ => false,
        };
        assert!(ok, "{name}: {e:?}");
        n += 1;
    }
    assert_eq!(n, 5);
}

#[test]
fn report_parser_rejects_duplicates_unsorted_and_noncanonical() {
    let s = text(&valid());
    let routes: Vec<&str> = s.lines().filter(|l| l.starts_with("route=")).collect();
    assert!(routes.len() >= 2);
    let dup = s.replacen(routes[0], &format!("{}\n{}", routes[0], routes[0]), 1);
    expect_err("duplicate_route_line", dup.as_bytes(), |e| matches!(e, ParseError::DuplicateLine(_)));
    let swapped = s
        .replacen(routes[0], "@@A", 1)
        .replacen(routes[1], routes[0], 1)
        .replacen("@@A", routes[1], 1);
    expect_err("unsorted_route_lines", swapped.as_bytes(), |e| matches!(e, ParseError::UnsortedLine(_)));
    let noncanon = s.replacen("hub_addr=192.168.1.10", "hub_addr=192.168.001.10", 1);
    expect_err("hub_addr_leading_zero", noncanon.as_bytes(), |e| matches!(e, ParseError::Value { .. }));
    let gen0 = s.replacen("generation=1", "generation=01", 1);
    expect_err("generation_leading_zero", gen0.as_bytes(), |e| matches!(e, ParseError::Value { .. }));
}

#[test]
fn parser_is_strict_on_boundaries_the_vectors_do_not_list() {
    let b = valid();
    let s = text(&b);
    let mut no_lf = b.clone();
    no_lf.pop();
    expect_err("missing_final_lf", &no_lf, |e| matches!(e, ParseError::MissingFinalLf));
    expect_err("cr", s.replacen('\n', "\r\n", 1).as_bytes(), |e| matches!(e, ParseError::Cr));
    let mut bom = vec![0xEF, 0xBB, 0xBF];
    bom.extend_from_slice(&b);
    expect_err("bom", &bom, |e| matches!(e, ParseError::Bom));
    expect_err("blank_line", s.replacen('\n', "\n\n", 1).as_bytes(), |e| matches!(e, ParseError::BlankLine(_)));
    expect_err("trailing_space", s.replacen("format=1\n", "format=1 \n", 1).as_bytes(), |e| matches!(e, ParseError::TrailingWhitespace(_)));
    expect_err("unknown_key", format!("{s}extra=1\n").as_bytes(), |e| matches!(e, ParseError::UnknownKey(_)));
    let max_active: String = s.lines().find(|l| l.starts_with("max_active=")).unwrap().to_owned();
    expect_err("missing_key", s.replacen(&format!("{max_active}\n"), "", 1).as_bytes(), |e| matches!(e, ParseError::MissingKey("max_active")));
    let nonce = s.lines().nth(2).unwrap().to_owned();
    let boot = s.lines().nth(3).unwrap().to_owned();
    let ooo = s.replacen(&format!("{nonce}\n{boot}\n"), &format!("{boot}\n{nonce}\n"), 1);
    expect_err("out_of_order_key", ooo.as_bytes(), |e| matches!(e, ParseError::OutOfOrder(_)));
    expect_err("duplicate_single_key", s.replacen("format=1\n", "format=1\nformat=1\n", 1).as_bytes(), |e| matches!(e, ParseError::DuplicateKey(_)));
    let route = s.lines().find(|l| l.starts_with("route=")).unwrap().to_owned();
    expect_err("wrong_field_count", s.replacen(&route, &format!("{route}|9"), 1).as_bytes(), |e| matches!(e, ParseError::Value { err: EncodingError::FieldCount { .. }, .. }));
    expect_err("truncated_escape", s.replacen("|eth0\n", "|eth%4\n", 1).as_bytes(), |e| matches!(e, ParseError::Value { err: EncodingError::TruncatedEscape, .. }));
    expect_err("raw_non_ascii_valid_utf8", s.replacen("|eth0\n", "|\u{e9}th0\n", 1).as_bytes(), |e| matches!(e, ParseError::Value { err: EncodingError::RawByte(0xC3), .. }));
    expect_err("format_2", s.replacen("format=1", "format=2", 1).as_bytes(), |e| matches!(e, ParseError::BadFormat(_)));
    expect_err("empty_input", b"", |e| matches!(e, ParseError::MissingFinalLf));
}

#[test]
fn invalid_utf8_is_rejected_as_its_own_step_before_any_grammar() {
    let s = valid();
    for (name, bad) in [("lone_0xff", vec![0xFFu8]), ("overlong_c0_80", vec![0xC0, 0x80])] {
        let mut b = s.clone();
        let at = b.iter().position(|&c| c == b'x').unwrap_or(10); // inside "complete=..." or a key; any ASCII byte
        b.splice(at..=at, bad);
        let e = parse_report(&b, DEFAULT).expect_err(name);
        println!("REJECT {name}: {e}");
        assert!(matches!(e, ParseError::NotUtf8(_)), "{e:?}");
    }
    // Observation: every byte the grammar admits is ASCII, so the UTF-8 step is logically
    // subsumed by the raw-byte rule; it is kept here only to give a distinct error.
    assert!(s.is_ascii());
}

fn incomplete_bytes() -> Vec<u8> {
    let mut rig = Rig::new();
    rig.toml = "[safety]\nmanagement_cidrs = [\"not-a-cidr\"]\n".into();
    rig.attest_raw([9; 32], T0, BOOT_A).expect("signed").report
}

#[test]
fn incomplete_reason_empty_value_conflict_is_visible_in_three_readings() {
    let ok = valid();
    let bad = incomplete_bytes();
    let s = text(&bad);
    assert!(s.contains("complete=false\nincomplete_reason=config_invalid\n"));
    // Reading A (harness default): empty exactly when complete=true.
    assert!(parse_report(&ok, EmptyReasonPolicy::EmptyIffComplete).is_ok());
    assert!(parse_report(&bad, EmptyReasonPolicy::EmptyIffComplete).is_ok());
    // Reading B (04b section 1 literal, no value may be empty): every complete report dies.
    let e = parse_report(&ok, EmptyReasonPolicy::RejectAllEmpty).expect_err("literal 04b");
    println!("REJECT complete_report_under_literal_04b: {e}");
    assert!(matches!(e, ParseError::EmptyValue("incomplete_reason")));
    assert!(parse_report(&bad, EmptyReasonPolicy::RejectAllEmpty).is_ok());
    // Reading C (accept anywhere): the pair complete/reason can disagree.
    let lie = text(&ok).replacen("complete=true", "complete=false", 1);
    assert!(parse_report(lie.as_bytes(), EmptyReasonPolicy::EmptyIffComplete).is_err());
    assert!(parse_report(lie.as_bytes(), EmptyReasonPolicy::AcceptAnywhere).is_ok());
    // Other combinations are inconsistent under A.
    let both = text(&ok).replacen("incomplete_reason=", "incomplete_reason=x", 1);
    assert!(matches!(parse_report(both.as_bytes(), DEFAULT), Err(ParseError::ReasonInconsistent(_))));
}

#[test]
fn no_other_key_may_carry_an_empty_value() {
    let s = text(&valid());
    let keys: Vec<String> = s.lines().map(|l| l.split_once('=').unwrap().0.to_owned()).collect();
    let mut n = 0;
    for key in keys.iter().filter(|k| *k != "incomplete_reason") {
        let line = s.lines().find(|l| l.starts_with(&format!("{key}="))).unwrap();
        let blank = s.replacen(line, &format!("{key}="), 1);
        let e = parse_report(blank.as_bytes(), DEFAULT).expect_err(key);
        assert!(matches!(e, ParseError::EmptyValue(_)), "{key}: {e:?}");
        n += 1;
    }
    println!("EMPTY-VALUE keys rejected: {n}");
}

#[test]
fn empty_incomplete_reason_does_not_move_the_hashes() {
    // incomplete_reason is excluded from both hashes (04b section 3); the choice of reading
    // changes the signed bytes only.
    let r = parse_report(&valid(), DEFAULT).unwrap();
    let mut r2 = r.clone();
    r2.complete = false;
    r2.incomplete_reason = Some("x".into());
    assert_eq!(r.config.hash(), r2.config.hash());
    assert_eq!(r.routes.hash(), r2.routes.hash());
    assert_ne!(r.render(), r2.render());
}
