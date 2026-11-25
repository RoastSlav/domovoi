//! The field codec and the selector's inner escaping (decision D-46).

use domovoi_core::attest::{
    EncodeError, Family, Rule, Selector, check_ifname, decode_field, encode_field, is_literal,
};

use crate::common::{rule, vector_text};

fn selector(kind: &str, name: Vec<u8>) -> Selector {
    let mut selector = Selector {
        not: true,
        ..Selector::default()
    };
    match kind {
        "iif" => selector.iif = Some(name),
        _ => selector.oif = Some(name),
    }
    selector
}

fn rows() -> Vec<(String, Vec<u8>, String)> {
    vector_text("selector_escaping.txt")
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|row| {
            let mut parts = row.split('\t');
            let kind = parts.next().expect("kind").to_owned();
            let name = hex::decode(parts.next().expect("name")).expect("hex");
            (kind, name, parts.next().expect("line").to_owned())
        })
        .collect()
}

#[test]
fn escaping_vectors_are_frozen() {
    let rows = rows();
    assert_eq!(rows.len(), 12);
    for (kind, name, line) in rows {
        let produced = rule(Family::Inet, 200, selector(&kind, name.clone()), 200);
        assert_eq!(produced, line, "{kind} {name:?}");
        let parsed = Rule::parse(&line).expect("the frozen line parses");
        assert_eq!(parsed.selector, selector(&kind, name));
        assert_eq!(parsed.render(), line);
    }
}

#[test]
fn selector_encoding_is_injective_for_names_with_separators() {
    // Without inner escaping `iif=a` plus `not` and the name `a;not` rendered the same text.
    let flagged = Selector {
        iif: Some(b"a".to_vec()),
        not: true,
        ..Selector::default()
    };
    let named = Selector {
        iif: Some(b"a;not".to_vec()),
        ..Selector::default()
    };
    assert_eq!(flagged.render(), "iif=a;not");
    assert_eq!(named.render(), "iif=a%3Bnot");
    assert_ne!(flagged.render(), named.render());
    assert_eq!(Selector::parse(&named.render()).expect("named"), named);
    assert_eq!(
        Selector::parse(&flagged.render()).expect("flagged"),
        flagged
    );
    // A name holding `=` or `|` cannot become another item or another field.
    for name in [
        b"a=b".to_vec(),
        b"a|b".to_vec(),
        b"a,b".to_vec(),
        b"a\\b".to_vec(),
    ] {
        let selector = Selector {
            iif: Some(name.clone()),
            ..Selector::default()
        };
        let line = rule(Family::Inet, 1, selector.clone(), 1);
        assert_eq!(line.matches('|').count(), 3, "{name:?}");
        assert_eq!(Rule::parse(&line).expect("parses").selector, selector);
    }
}

#[test]
fn r4_digest_unchanged_by_inner_escaping() {
    let line = rule(Family::Inet, 200, selector("iif", b"br-lan".to_vec()), 200);
    assert_eq!(line, "inet|200|iif%3Dbr-lan%3Bnot|200");
    // A name made only of literal characters is unchanged, so R4's digest is the published one;
    // the digest itself is asserted by `r_vectors_match_published_digests`.
    assert_eq!(encode_field(b"br-lan"), "br-lan");
}

#[test]
fn oif_selector_escaping_matches_iif() {
    let rows = rows();
    for (kind, name, line) in rows.iter().filter(|(kind, ..)| kind == "iif") {
        let counterpart = rows
            .iter()
            .find(|(other, other_name, _)| other == "oif" && other_name == name)
            .expect("an oif row for every iif row");
        assert_eq!(
            counterpart.2,
            line.replacen("iif", "oif", 1),
            "{kind} {name:?}"
        );
    }
}

#[test]
fn selector_refuses_noncanonical_items() {
    for text in [
        "iif=",
        "iif=a;iif=b",
        "not;from=10.0.0.0/8",
        "to=10.0.0.0/8;from=10.0.0.0/8",
        "ipproto=256",
        "ipproto=06",
        "sport=10-5",
        "sport=1-65536",
        "dport=0-70000",
        "uidrange=5-4294967296",
        "fwmark=0x01/0xff",
        "fwmark=0xFF/0xff",
        "fwmark=0x1/0x123456789",
        "fwmark=1/0xff",
        "from=10.0.0.7/8",
        "iif=a b",
        "iif=%61",
        "iif=0123456789abcdef",
        "all;not",
        "NOT",
        "",
    ] {
        assert!(Selector::parse(text).is_err(), "{text:?}");
    }
    let full = "from=10.0.0.0/8;to=192.0.2.0/24;iif=a;oif=b;fwmark=0x0/0xff;ipproto=255;sport=1-65535;dport=0-0;uidrange=0-4294967295;not";
    assert_eq!(
        Selector::parse(full).expect("a full selector").render(),
        full
    );
}

#[test]
fn field_codec_refuses_every_non_canonical_form() {
    assert_eq!(decode_field("eth%7C0%20x").expect("valid"), b"eth|0 x");
    assert!(matches!(
        decode_field("eth%7c0"),
        Err(EncodeError::LowercaseHex(_))
    ));
    assert!(matches!(
        decode_field("%65th0"),
        Err(EncodeError::EncodedLiteral(_))
    ));
    assert!(matches!(
        decode_field("eth%4"),
        Err(EncodeError::TruncatedEscape)
    ));
    assert!(matches!(
        decode_field("eth%"),
        Err(EncodeError::TruncatedEscape)
    ));
    assert!(matches!(
        decode_field("eth%zz"),
        Err(EncodeError::BadHex(_))
    ));
    assert!(matches!(
        decode_field("eth|0"),
        Err(EncodeError::RawByte(b'|'))
    ));
    assert!(matches!(
        decode_field("\u{e9}"),
        Err(EncodeError::RawByte(0xC3))
    ));
    assert!(matches!(
        decode_field("a b"),
        Err(EncodeError::RawByte(b' '))
    ));
    for byte in 0..=255_u8 {
        assert_eq!(
            is_literal(byte),
            byte.is_ascii_alphanumeric() || b"._:/@-".contains(&byte),
            "{byte}"
        );
    }
}

#[test]
fn interface_names_are_one_to_fifteen_octets() {
    assert!(check_ifname(b"").is_err());
    assert!(check_ifname(&[b'a'; 15]).is_ok());
    assert!(check_ifname(&[b'a'; 16]).is_err());
    // The limit is on the decoded name, not the encoded text.
    assert!(check_ifname(&decode_field(&"%3B".repeat(15)).expect("valid")).is_ok());
}
