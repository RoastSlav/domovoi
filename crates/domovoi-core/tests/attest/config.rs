//! Effective configuration: normalisation, defaults and the values that give no hash.

use domovoi_core::attest::{
    Cidr, ConfigError, ConfigLines, IncompleteReason, MANDATORY_UNITS, SafetyInput, canonical_unit,
    check_container, parse_max_ttl,
};

fn units(names: &[&str]) -> SafetyInput {
    SafetyInput {
        protected_units: names.iter().map(|n| (*n).to_owned()).collect(),
        ..SafetyInput::default()
    }
}

#[test]
fn mandatory_units_cannot_be_removed() {
    for input in [
        SafetyInput::default(),
        units(&[]),
        units(&["only-this.service"]),
    ] {
        let config = ConfigLines::effective(&input).expect("valid");
        for unit in MANDATORY_UNITS {
            assert!(config.protected_unit.iter().any(|u| u == unit), "{unit}");
        }
    }
    assert_eq!(MANDATORY_UNITS.len(), 4);
}

#[test]
fn unit_names_with_backslash_are_refused_as_config_invalid() {
    let err = ConfigLines::effective(&units(&["dev-sda\\x2d1.device"])).expect_err("refused");
    assert!(matches!(err, ConfigError::Invalid(_)));
    assert!(canonical_unit("a\\b").is_err());
    for bad in ["", "a b", "a|b", "a/b", "a=b", "a%41", "é"] {
        assert!(canonical_unit(bad).is_err(), "{bad:?}");
    }
    assert!(canonical_unit(&"a".repeat(256)).is_err());
    assert!(canonical_unit(&"a".repeat(244)).is_ok());
}

#[test]
fn unit_names_get_the_service_suffix_only_without_a_known_type() {
    for (given, canonical) in [
        ("plex", "plex.service"),
        ("plex.service", "plex.service"),
        ("a.socket", "a.socket"),
        ("a.b", "a.b.service"),
        ("a@b", "a@b.service"),
        (".service", ".service.service"),
        ("x.Service", "x.Service.service"),
        ("sys-x.mount", "sys-x.mount"),
    ] {
        assert_eq!(canonical_unit(given).as_deref(), Ok(canonical), "{given}");
    }
}

#[test]
fn max_ttl_zero_is_config_invalid() {
    for zero in ["0s", "0m", "0h", "0d", "00s"] {
        assert!(parse_max_ttl(zero).is_err(), "{zero}");
        let input = SafetyInput {
            max_ttl: Some(zero.to_owned()),
            ..SafetyInput::default()
        };
        assert!(ConfigLines::effective(&input).is_err(), "{zero}");
    }
}

#[test]
fn max_ttl_grammar_is_digits_and_one_lowercase_unit() {
    for (text, seconds) in [
        ("1s", 1),
        ("90s", 90),
        ("2m", 120),
        ("24h", 86_400),
        ("7d", 604_800),
    ] {
        assert_eq!(parse_max_ttl(text), Ok(seconds), "{text}");
    }
    for bad in [
        "", "h", "1", "-1h", "+1h", "1H", "1.5h", "1h30m", "1 h", " 1h", "1h ", "1w", "٣h",
    ] {
        assert!(parse_max_ttl(bad).is_err(), "{bad:?}");
    }
    assert!(
        parse_max_ttl("213503982334602d").is_err(),
        "overflow of u64 seconds"
    );
    assert!(parse_max_ttl("99999999999999999999999s").is_err());
}

#[test]
fn defaults_apply_when_nothing_is_configured() {
    let config = ConfigLines::effective(&SafetyInput::default()).expect("valid");
    assert_eq!((config.max_ttl_s, config.max_active), (3600, 0));
    let input = SafetyInput {
        max_active: Some(0),
        max_ttl: Some("1h".into()),
        ..SafetyInput::default()
    };
    assert_eq!(ConfigLines::effective(&input).expect("valid"), config);
}

#[test]
fn cidrs_are_normalised_and_deduplicated() {
    let input = SafetyInput {
        management_cidrs: vec![
            "192.0.2.77/24".into(),
            "192.0.2.0/24".into(),
            "2001:DB8:0:0:0:0:0:1/32".into(),
            "2001:db8::/32".into(),
        ],
        ..SafetyInput::default()
    };
    let config = ConfigLines::effective(&input).expect("valid");
    assert_eq!(config.cidr, ["192.0.2.0/24", "2001:db8::/32"]);
    for bad in [
        "192.0.2.0",
        "192.0.2.0/",
        "192.0.2.0/33",
        "192.0.2.0/+8",
        "192.0.2.0/ 8",
        "host/8",
        "2001:db8::/129",
        "/8",
    ] {
        let input = SafetyInput {
            management_cidrs: vec![bad.into()],
            ..SafetyInput::default()
        };
        assert!(ConfigLines::effective(&input).is_err(), "{bad}");
        assert!(Cidr::parse_config(bad).is_err(), "{bad}");
    }
}

#[test]
fn container_names_follow_the_docker_grammar() {
    for good in ["a", "ctr-a", "A.b_c-1", &"a".repeat(255)] {
        check_container(good).unwrap_or_else(|err| panic!("{good}: {err}"));
    }
    for bad in ["", "-a", ".a", "_a", "a b", "a/b", "a:b", &"a".repeat(256)] {
        assert!(check_container(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn config_errors_sign_as_config_invalid() {
    assert_eq!(IncompleteReason::CONFIG_INVALID, "config_invalid");
    assert!(
        IncompleteReason::new(IncompleteReason::CONFIG_INVALID)
            .expect("a token")
            .is_known()
    );
    assert!(IncompleteReason::new("Config").is_none());
    assert!(IncompleteReason::new("").is_none());
    for token in IncompleteReason::KNOWN {
        assert!(IncompleteReason::new(token).is_some(), "{token}");
    }
}

#[test]
fn cidr_set_algebra_is_family_aware() {
    let v4 = Cidr::parse_report("192.0.2.0/24").expect("v4");
    let host = Cidr::host("192.0.2.9".parse().expect("ip"));
    let v6 = Cidr::parse_report("::/0").expect("v6");
    assert!(host.subset_of(&v4) && !v4.subset_of(&host));
    assert!(!host.subset_of(&v6), "families never mix");
    assert!(v4.overlaps(&host) && host.overlaps(&v4));
    assert!(!v4.overlaps(&Cidr::parse_report("198.51.100.0/24").expect("other")));
    assert!(Cidr::parse_report("0.0.0.0/0").expect("all").overlaps(&v4));
}

#[test]
fn restart_units_are_signed_sorted_and_change_the_hash() {
    let none = ConfigLines::effective(&SafetyInput::default()).expect("valid");
    assert!(none.restart_unit.is_empty());
    assert!(none.lines().iter().all(|l| !l.starts_with("restart_unit=")));

    let input = SafetyInput {
        restart_units: vec![
            "nginx".to_owned(),
            "cron.service".to_owned(),
            "nginx".to_owned(),
        ],
        ..SafetyInput::default()
    };
    let with = ConfigLines::effective(&input).expect("valid");
    assert_eq!(with.restart_unit, ["cron.service", "nginx.service"]);
    assert!(
        with.lines()
            .iter()
            .any(|l| l == "restart_unit=cron.service")
    );
    assert_ne!(
        with.hash(),
        none.hash(),
        "the list is part of what is signed"
    );

    let bad = SafetyInput {
        restart_units: vec!["a b".to_owned()],
        ..SafetyInput::default()
    };
    assert!(ConfigLines::effective(&bad).is_err());
}
