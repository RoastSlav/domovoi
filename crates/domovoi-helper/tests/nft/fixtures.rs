//! `nft -j` listings and error text recorded from real `nft`, one directory per version under
//! `tests/fixtures/nft/`, made by `record.sh` in a throwaway namespace with documentation
//! addresses only. Add a directory per version a host is validated on; every test here runs on
//! each one.

use std::path::{Path, PathBuf};

use domovoi_core::containment::{NftOp, StderrClass, Token, classify_stderr};
use domovoi_helper::nft::view::{is_absent, parse};

fn versions() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/nft");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root)
        .expect("fixtures")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    assert!(!dirs.is_empty(), "at least one recorded version");
    dirs
}

fn read(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join(name))
        .unwrap_or_else(|e| panic!("{}: {e}", dir.join(name).display()))
}

#[test]
fn list_members_parses_json_fixture_per_nft_version() {
    for dir in versions() {
        let view = parse(&read(&dir, "list_table.json")).expect("a listing");
        let host = &view.sets["blocked4"].members;
        assert_eq!(host.len(), 1, "{}", dir.display());
        assert_eq!(
            (host[0].lo, host[0].hi),
            (0xCB00_7107, 0xCB00_7107),
            "203.0.113.7"
        );
        assert!(host[0].expires.is_some_and(|e| (1..=30).contains(&e)));
        let net = &view.sets["blockednet4"].members;
        assert_eq!(
            (net[0].lo, net[0].hi),
            (0xC633_6400, 0xC633_64FF),
            "198.51.100.0/24"
        );
        let excl = &view.sets["excl4"].members;
        assert_eq!(
            (excl[0].lo, excl[0].hi),
            (0xC000_0200, 0xC000_02FF),
            "192.0.2.0/24"
        );
        assert_eq!(view.sets["blocked4"].size, Some(2));
        assert_eq!(view.sets["blocked4"].timeout, Some(30));
        let chain = view.chain.as_ref().expect("the chain");
        assert_eq!(
            (chain.hook.as_str(), chain.prio, chain.policy.as_str()),
            ("prerouting", -310, "accept")
        );
        assert_eq!(
            view.rules,
            [
                "counter",
                "ip.saddr@excl4:accept",
                "ip.saddr@blocked4:counter,drop",
                "ip.saddr@blockednet4:counter,drop"
            ]
        );
        assert_eq!(
            view.counters.len(),
            3,
            "the arrival counter and the two drop counters"
        );
    }
}

#[test]
fn error_token_mapping_matches_recorded_stderr_per_nft_version() {
    let cases = [
        (
            "create_exists",
            NftOp::CreateElement,
            StderrClass::AlreadyPresent,
            Some(Token::AlreadyPresent),
        ),
        (
            "delete_absent",
            NftOp::Confirm,
            StderrClass::ElementAbsent,
            None,
        ),
        (
            "delete_interval_absent",
            NftOp::Rollback,
            StderrClass::ElementAbsent,
            None,
        ),
        (
            "add_interval_overlap",
            NftOp::Rollback,
            StderrClass::IntervalOverlap,
            Some(Token::CoveredByOther),
        ),
        (
            "create_table_exists",
            NftOp::CreateTable,
            StderrClass::TableExistsRace,
            Some(Token::TableExistsRace),
        ),
        (
            "set_full",
            NftOp::CreateElement,
            StderrClass::SetFull,
            Some(Token::SetFull),
        ),
        (
            "missing_table_script",
            NftOp::Confirm,
            StderrClass::TableOrSetMissing,
            None,
        ),
        (
            "missing_table",
            NftOp::Read,
            StderrClass::TableOrSetMissing,
            None,
        ),
    ];
    for dir in versions() {
        for (name, op, class, token) in cases {
            let text = read(&dir, &format!("{name}.err"));
            assert_eq!(
                classify_stderr(op, &text),
                class,
                "{} {name}",
                dir.display()
            );
            assert_eq!(
                class.token(matches!(op, NftOp::Rollback), true),
                token,
                "{name}"
            );
        }
        assert!(is_absent(Some(1), &read(&dir, "missing_table.err")));
        assert!(!is_absent(Some(1), &read(&dir, "create_exists.err")));
    }
}

#[test]
fn bare_no_such_file_is_never_read_as_an_absent_element() {
    for dir in versions() {
        for name in ["missing_table", "missing_table_script"] {
            let text = read(&dir, &format!("{name}.err"));
            assert!(!text.contains("Could not process rule"), "{name}");
            assert_eq!(
                classify_stderr(NftOp::Confirm, &text),
                StderrClass::TableOrSetMissing,
                "{name}: only the table check can say what this means"
            );
        }
    }
}
