//! Tests for SELFTEST-1: spellings read from hand-written and macro-declared enums, and the
//! `// covers:` comments that claim them.

use std::path::Path;

use super::*;

const TOKENS_MACRO: &str = r#"
//! Tokens.
macro_rules! tokens {
    ($name:ident { $($variant:ident => $text:literal,)+ }) => {
        pub enum $name { $($variant,)+ }
        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant,)+];
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text,)+ }
            }
        }
    };
}
tokens! {
    /// Why a request was refused.
    RefusalToken {
        /// Bad permissions.
        HelperStatePermissions => "helper_state_permissions",
        Uninitialized => "uninitialized",
    }
}
tokens! {
    CheckToken {
        PrivilegeBoundaryNotInEffect => "privilege_boundary_not_in_effect",
    }
}
"#;

const CHECK_MACRO: &str = r#"
use crate::identity::CheckToken;
macro_rules! check_ids {
    ($($variant:ident => $text:literal, $run_by:ident;)+) => {
        pub enum CheckId { $($variant,)+ }
        impl CheckId {
            pub const ALL: &'static [Self] = &[$(Self::$variant,)+];
            pub const fn as_str(self) -> &'static str { match self { $(Self::$variant => $text,)+ } }
        }
    };
}
check_ids! {
    St01 => "ST-01", Agent;
    St02 => "ST-02", Agent;
}
impl std::fmt::Display for CheckId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(self.as_str()) }
}
"#;

const HAND_WRITTEN: &str = r#"
pub enum CheckId { St01, St02 }
impl CheckId {
    pub const ALL: &'static [Self] = &[Self::St01, Self::St02];
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::St01 => "ST-01",
            Self::St02 => "ST-02",
        }
    }
}
"#;

const ALL_COVERED: &str = r#"
// covers: ST-01, ST-02
#[test]
fn checks_are_named() {}

// covers: helper_state_permissions uninitialized
// covers: privilege_boundary_not_in_effect
#[tokio::test]
async fn tokens_are_named() {}
"#;

fn sources(enum_files: &[&str], tests: &[&str]) -> Sources {
    let own = |files: &[&str], prefix: &str| {
        files
            .iter()
            .enumerate()
            .map(|(i, t)| (format!("{prefix}{i}.rs"), t.to_string()))
            .collect()
    };
    Sources {
        enum_files: own(enum_files, "crates/domovoi-core/src/e"),
        test_files: own(tests, "crates/domovoi-helper/tests/t"),
    }
}

fn run(sources: &Sources) -> Vec<Violation> {
    match check(sources) {
        Outcome::Ran(found) => found,
        Outcome::Skipped(note) => panic!("unexpected skip: {note}"),
    }
}

#[test]
fn every_selftest_check_has_a_test() {
    let complete = sources(&[TOKENS_MACRO, CHECK_MACRO], &[ALL_COVERED]);
    assert!(run(&complete).is_empty(), "{:?}", run(&complete));

    // One check, one refusal token and one check token without a claim.
    let partial = "\n// covers: ST-01\n#[test]\nfn one() {}\n// covers: uninitialized\n#[test]\nfn two() {}\n";
    let found = run(&sources(&[TOKENS_MACRO, CHECK_MACRO], &[partial]));
    let missing: Vec<&str> = found
        .iter()
        .map(|v| v.message.split('`').nth(3).unwrap_or_default())
        .collect();
    assert_eq!(
        missing,
        [
            "ST-02",
            "privilege_boundary_not_in_effect",
            "helper_state_permissions"
        ],
        "{found:?}"
    );
    assert!(found.iter().all(|v| v.rule == "SELFTEST-1"));
}

#[test]
fn spellings_come_from_a_hand_written_as_str_too() {
    let found = run(&sources(&[HAND_WRITTEN, TOKENS_MACRO], &[]));
    let named: Vec<&str> = found
        .iter()
        .map(|v| v.message.split('`').nth(3).unwrap_or_default())
        .collect();
    assert_eq!(
        named,
        [
            "ST-01",
            "ST-02",
            "privilege_boundary_not_in_effect",
            "helper_state_permissions",
            "uninitialized"
        ]
    );
}

#[test]
fn a_covers_comment_must_sit_above_a_test_function() {
    let stray = "\n// covers: ST-01 ST-02\nfn helper() {}\n\n// covers: uninitialized\n#[derive(Debug)]\nstruct NotATest;\n";
    let found = run(&sources(&[CHECK_MACRO, TOKENS_MACRO], &[stray]));
    assert_eq!(found.len(), 5, "nothing is claimed: {found:?}");
}

#[test]
fn a_missing_enum_is_a_violation_and_missing_files_are_a_skip() {
    let found = run(&sources(&[CHECK_MACRO], &[ALL_COVERED]));
    let names: Vec<&str> = found
        .iter()
        .map(|v| v.message.split('`').nth(1).unwrap_or_default())
        .collect();
    assert_eq!(names, ["CheckToken", "RefusalToken"], "{found:?}");
    assert!(matches!(check(&Sources::default()), Outcome::Skipped(_)));
}

#[test]
fn the_real_enums_are_read_and_covered() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root");
    let sources = load(root).expect("the sources read");
    match check(&sources) {
        Outcome::Skipped(note) => assert!(note.contains("SELFTEST-1"), "{note}"),
        Outcome::Ran(found) => assert!(found.is_empty(), "{found:#?}"),
    }
}
