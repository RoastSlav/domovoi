//! The strict `[safety]` reader: the frozen config vectors from their TOML inputs, and every
//! way `helper.toml` can give no effective set.

#[cfg(test)]
mod strict_safety {
    use domovoi_config::{HelperSafetyError, effective_config_lines, parse_safety_input};
    use domovoi_core::attest::{CONFIG_PREFIX, Hash32, domain_bytes};

    fn vector(name: &str) -> Vec<u8> {
        let path = format!(
            "{}/../domovoi-core/tests/attest/vectors/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read(&path).unwrap_or_else(|err| panic!("{path}: {err}"))
    }

    fn text(name: &str) -> String {
        String::from_utf8(vector(name)).expect("utf-8")
    }

    fn bytes_of(input: &str) -> Vec<u8> {
        let config = effective_config_lines(&text(input)).expect("a valid configuration");
        domain_bytes(CONFIG_PREFIX, &config.lines())
    }

    const C1: &str = "96f1584f01f025fa82b1e57c3ebf8d220b87d6d88543d08add3dc273cbf4b9ea";
    const C3: &str = "494a37a86856bb2b2f6e80e13efc654e8b7fbaf9e73245277f6d34993d42dc4f";
    const C4: &str = "7f1db0e04eb12b4211166be055486a71e44be9e00c2a089cc9e51f4f06556baa";

    fn check(input: &str, fixture: &str, length: usize, digest: &str) {
        let produced = bytes_of(input);
        assert_eq!(produced.len(), length, "{input}");
        assert_eq!(produced, vector(fixture), "{input}");
        assert_eq!(Hash32::of(&produced).to_string(), digest, "{input}");
    }

    #[test]
    fn config_hash_matches_vectors_through_toml() {
        check("c1_input.toml", "c1.txt", 320, C1);
        check("c2_input.toml", "c2.txt", 320, C1);
        check("c3_input.toml", "c3.txt", 320, C3);
        check("c4_input.toml", "c4.txt", 198, C4);
    }

    #[test]
    fn equivalent_toml_same_config_hash() {
        let (a, b) = (text("c1_input.toml"), text("c2_input.toml"));
        assert_ne!(a, b, "the inputs differ as text");
        let (a, b) = (
            effective_config_lines(&a).expect("c1"),
            effective_config_lines(&b).expect("c2"),
        );
        assert_eq!(a, b);
        assert_eq!(a.hash(), b.hash());
    }

    fn reason(toml: &str) -> &'static str {
        effective_config_lines(toml).expect_err(toml).reason()
    }

    #[test]
    fn unknown_safety_key_is_config_invalid() {
        assert_eq!(reason("[safety]\nmanagement_cidr = []"), "config_invalid");
        assert_eq!(
            reason("[safety]\nprotected_units = []\nextra = 1"),
            "config_invalid"
        );
        assert_eq!(
            reason("[safety.containment]\nmax_ttl = \"1h\"\nmax_activ = 1"),
            "config_invalid"
        );
        assert_eq!(reason("[safety.other]\nx = 1"), "config_invalid");
    }

    #[test]
    fn wrong_types_and_bad_values_are_config_invalid() {
        for toml in [
            "safety = 3",
            "[safety]\nmanagement_cidrs = \"192.0.2.0/24\"",
            "[safety]\nmanagement_cidrs = [1]",
            "[safety]\nmanagement_cidrs = [\"not-a-cidr\"]",
            "[safety]\nmanagement_cidrs = [\"192.0.2.0\"]",
            "[safety]\nprotected_units = [\"a\\\\b\"]",
            "[safety]\nprotected_containers = [\"-bad\"]",
            "[safety.containment]\nmax_ttl = \"0s\"",
            "[safety.containment]\nmax_ttl = \"1h30m\"",
            "[safety.containment]\nmax_ttl = \"1H\"",
            "[safety.containment]\nmax_ttl = 3600",
            "[safety.containment]\nmax_active = -1",
            "[safety.containment]\nmax_active = \"2\"",
            "[safety.containment]\nmax_active = 1.5",
        ] {
            assert_eq!(reason(toml), "config_invalid", "{toml}");
        }
    }

    #[test]
    fn text_that_is_not_toml_is_config_unreadable() {
        for toml in [
            "[safety",
            "safety = ",
            "= 1",
            "[safety]\nmanagement_cidrs = [\"a\"",
        ] {
            let err = effective_config_lines(toml).expect_err(toml);
            assert!(matches!(err, HelperSafetyError::Unreadable(_)), "{toml}");
            assert_eq!(err.reason(), "config_unreadable");
        }
    }

    #[test]
    fn a_missing_safety_section_equals_an_empty_one_and_other_tables_are_ignored() {
        let empty = effective_config_lines("").expect("empty");
        assert_eq!(empty, effective_config_lines("[safety]").expect("safety"));
        assert_eq!(
            empty,
            effective_config_lines(&text("c4_input.toml")).expect("c4")
        );
        let others = "groups = [\"net.read\"]\n[limits]\nmutations_per_hour = 3\n[nft]\ntable = \"x\"\n[surprise]\nanything = true\n";
        assert_eq!(effective_config_lines(others).expect("others"), empty);
        assert_eq!(empty.protected_unit.len(), 4);
    }

    #[test]
    fn raw_values_are_handed_over_as_written() {
        let input = parse_safety_input(&text("c1_input.toml")).expect("c1");
        assert_eq!(input.management_cidrs[0], "192.168.1.7/24");
        assert_eq!(input.max_ttl.as_deref(), Some("24h"));
        assert_eq!(input.max_active, Some(50));
        let none = parse_safety_input("").expect("empty");
        assert_eq!((none.max_ttl, none.max_active), (None, None));
    }
}

#[cfg(test)]
mod restart_units {
    use domovoi_config::effective_config_lines;

    #[test]
    fn the_systemd_restart_list_is_read_for_the_attestation() {
        let lines = effective_config_lines("[systemd]\nrestart_units = [\"nginx\"]\n").unwrap();
        assert_eq!(lines.restart_unit, ["nginx.service"]);
        let none = effective_config_lines("").unwrap();
        assert!(none.restart_unit.is_empty());
    }
}
