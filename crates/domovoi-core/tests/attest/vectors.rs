//! The frozen config and route-state vectors, the line grammar and address corner forms.

use domovoi_core::attest::{
    CONFIG_PREFIX, ConfigLines, Hash32, ROUTES_PREFIX, RouteLines, SafetyInput, check_line,
    domain_bytes, parse_ip,
};

use crate::common::{hash, r1_lines, r4_lines, vector_bytes, vector_path, vector_text};

const C1: &str = "96f1584f01f025fa82b1e57c3ebf8d220b87d6d88543d08add3dc273cbf4b9ea";
const C3: &str = "494a37a86856bb2b2f6e80e13efc654e8b7fbaf9e73245277f6d34993d42dc4f";
const C4: &str = "7f1db0e04eb12b4211166be055486a71e44be9e00c2a089cc9e51f4f06556baa";
const R1: &str = "082d1afdf657096d1b3a97215142239f6910b74f13f158fbdda7f4d9fd4d1515";
const R2: &str = "7eb46fcc67a00328df9e33578f42a4a890e6797223d6676a162c0b4ca1777ad9";
const R3: &str = "7947b31b2c89edab797c3f6a1251b42ca4452d64d06dd786d68570d721996ec8";
const R4: &str = "530d9a9e286ca63ee69147d0a7e0326bceb4199f114824f1969e4126e7ac925a";

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

/// The typed form of the TOML in `c1_input.toml`.
pub fn c1_input() -> SafetyInput {
    SafetyInput {
        management_cidrs: strings(&["192.168.1.7/24", "2001:DB8:0:0:0:0:0:0/32", "100.64.0.0/10"]),
        protected_units: strings(&["plex", "sshd.service"]),
        protected_containers: strings(&["homeassistant"]),
        max_ttl: Some("24h".to_owned()),
        max_active: Some(50),
    }
}

/// The typed form of the TOML in `c2_input.toml`: the same set written differently.
pub fn c2_input() -> SafetyInput {
    SafetyInput {
        management_cidrs: strings(&["100.64.0.0/10", "192.168.1.0/24", "2001:db8::/32"]),
        protected_units: strings(&["sshd.service", "plex.service", "domovoi-agent.service"]),
        protected_containers: strings(&["homeassistant", "homeassistant"]),
        max_ttl: Some("1440m".to_owned()),
        max_active: Some(50),
    }
}

fn c3_input() -> SafetyInput {
    let mut input = c1_input();
    input.management_cidrs[2] = "100.64.0.0/11".to_owned();
    input
}

fn config_bytes(input: &SafetyInput) -> Vec<u8> {
    let config = ConfigLines::effective(input).expect("a valid configuration");
    domain_bytes(CONFIG_PREFIX, &config.lines())
}

fn check(name: &str, produced: &[u8], fixture: &str, length: usize, digest: &str) {
    let expected = vector_bytes(fixture);
    assert_eq!(produced.len(), length, "{name}: length");
    assert_eq!(produced, expected.as_slice(), "{name}: bytes");
    assert_eq!(Hash32::of(produced), hash(digest), "{name}: digest");
}

#[test]
fn c_vectors_match_published_digests() {
    check("C1", &config_bytes(&c1_input()), "c1.txt", 320, C1);
    check("C2", &config_bytes(&c2_input()), "c2.txt", 320, C1);
    check("C3", &config_bytes(&c3_input()), "c3.txt", 320, C3);
    check(
        "C4",
        &config_bytes(&SafetyInput::default()),
        "c4.txt",
        198,
        C4,
    );
}

#[test]
fn c1_and_c2_are_the_same_effective_configuration() {
    let (c1, c2) = (
        ConfigLines::effective(&c1_input()).expect("c1"),
        ConfigLines::effective(&c2_input()).expect("c2"),
    );
    assert_ne!(c1_input(), c2_input());
    assert_eq!(c1, c2);
    assert_eq!(c1.hash(), c2.hash());
    let c3 = ConfigLines::effective(&c3_input()).expect("c3");
    assert_ne!(c1.hash(), c3.hash(), "narrowing a cidr changes the hash");
    let c4 = ConfigLines::effective(&SafetyInput::default()).expect("c4");
    assert_eq!((c4.max_ttl_s, c4.max_active), (3600, 0));
    assert!(c4.cidr.is_empty() && c4.protected_container.is_empty());
    assert_eq!(c4.protected_unit.len(), 4, "only the mandatory set");
}

fn route_bytes(lines: &RouteLines) -> Vec<u8> {
    domain_bytes(ROUTES_PREFIX, &lines.lines())
}

#[test]
fn r_vectors_match_published_digests() {
    check("R1", &route_bytes(&r1_lines("10.8.0.1")), "r1.txt", 583, R1);
    check(
        "R2",
        &route_bytes(&r1_lines("10.8.0.254")),
        "r2.txt",
        585,
        R2,
    );
    check("R3", &route_bytes(&RouteLines::default()), "r3.txt", 25, R3);
    check("R4", &route_bytes(&r4_lines()), "r4.txt", 123, R4);
    assert_eq!(
        route_bytes(&RouteLines::default()),
        b"domovoi-safety-routes-v1\n",
        "the prefix already ends in LF and gets no second one"
    );
}

#[test]
fn every_vector_line_passes_the_strict_line_grammar() {
    for name in ["r1.txt", "r2.txt", "r4.txt", "c1.txt", "c4.txt"] {
        for line in vector_text(name).lines().skip(1) {
            if line.starts_with("max_") {
                continue;
            }
            check_line(line).unwrap_or_else(|err| panic!("{name}: {line}: {err}"));
        }
    }
}

#[test]
fn vector_files_are_lf_only() {
    let dir = std::path::Path::new(&vector_path("")).to_path_buf();
    let mut seen = 0;
    for entry in std::fs::read_dir(dir).expect("the vectors exist") {
        let path = entry.expect("an entry").path();
        let bytes = std::fs::read(&path).expect("readable");
        assert!(!bytes.contains(&b'\r'), "{} has a CR", path.display());
        seen += 1;
    }
    assert!(seen >= 13, "the vector set is present");
}

#[test]
fn ipv6_corner_forms_are_canonical() {
    let mut rows = 0;
    for row in vector_text("ipv6_corners.txt")
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
    {
        let (input, expected) = row.split_once('\t').expect("a tab-separated row");
        match (parse_ip(input), expected) {
            (Err(_), "REFUSED") => {}
            (Ok(ip), canonical) if canonical != "REFUSED" => {
                assert_eq!(ip.to_string(), canonical, "{input}");
                assert_eq!(
                    input, canonical,
                    "an accepted form is its own canonical text"
                );
            }
            (got, _) => panic!("{input}: expected {expected}, got {got:?}"),
        }
        rows += 1;
    }
    assert!(rows >= 12);
}

#[test]
fn route_family_must_match_its_addresses() {
    use domovoi_core::attest::Route;
    assert!(Route::parse("inet|254|0.0.0.0/0|2001:db8::1|eth0|1").is_err());
    assert!(Route::parse("inet6|254|0.0.0.0/0|-|eth0|1").is_err());
    assert!(Route::parse("inet|254|0.0.0.0/0|-|eth0|1").is_ok());
}

#[test]
fn config_hash_matches_vectors() {
    for (name, input, digest) in [
        ("C1", c1_input(), C1),
        ("C2", c2_input(), C1),
        ("C3", c3_input(), C3),
    ] {
        let config = ConfigLines::effective(&input).expect("a valid configuration");
        assert_eq!(config.hash(), hash(digest), "{name}");
    }
}

#[test]
fn route_state_hash_matches_vectors() {
    for (name, lines, digest) in [
        ("R1", r1_lines("10.8.0.1"), R1),
        ("R2", r1_lines("10.8.0.254"), R2),
        ("R3", RouteLines::default(), R3),
        ("R4", r4_lines(), R4),
    ] {
        assert_eq!(lines.hash(), hash(digest), "{name}");
    }
}
