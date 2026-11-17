//! Reachability tests over real files in a temporary directory: production and test context,
//! dual-reachable files, Cargo target roots, `#[cfg]` forms and unresolvable wiring.

use std::collections::BTreeSet;
use std::path::Path;

use super::*;

/// Write `files` under a fresh directory and return it.
fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (rel, text) in files {
        let path = dir.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    dir
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

/// Explore `dir` from the given production and test roots.
fn run(dir: &Path, production: &[&str], test: &[&str]) -> Reach {
    let roots = Roots {
        production: set(production),
        test: set(test),
    };
    explore(&mut DiskSource::new(dir).unwrap(), &roots)
}

const LIB: &str = "crates/x/src/lib.rs";

#[test]
fn a_file_loaded_by_a_test_module_and_a_production_module_stays_production() {
    let dir = tree(&[
        (
            LIB,
            "#[cfg(test)]\n#[path = \"shared.rs\"]\nmod test_shared;\n\n#[path = \"shared.rs\"]\nmod production_shared;\n",
        ),
        ("crates/x/src/shared.rs", "fn shared() {}\n"),
    ]);
    let reach = run(dir.path(), &[LIB], &[]);
    assert!(reach.production.contains("crates/x/src/shared.rs"));
    assert!(reach.test.contains("crates/x/src/shared.rs"));
    assert!(
        !reach.test_only().contains("crates/x/src/shared.rs"),
        "a file production also loads is scanned as production"
    );
    assert!(reach.problems.is_empty(), "{:#?}", reach.problems);
}

#[test]
fn a_file_only_a_cfg_test_module_loads_is_test_only() {
    let dir = tree(&[
        (LIB, "#[cfg(test)]\nmod fixtures;\nmod real;\n"),
        ("crates/x/src/fixtures.rs", "fn f() {}\n"),
        ("crates/x/src/real.rs", "fn r() {}\n"),
    ]);
    let reach = run(dir.path(), &[LIB], &[]);
    assert_eq!(reach.test_only(), set(&["crates/x/src/fixtures.rs"]));
    assert!(reach.production.contains("crates/x/src/real.rs"));
}

#[test]
fn a_file_only_an_integration_test_target_loads_is_test_only() {
    let root = "crates/x/tests/it.rs";
    let dir = tree(&[
        (LIB, "fn l() {}\n"),
        (root, "mod common;\n"),
        ("crates/x/tests/common/mod.rs", "mod deeper;\n"),
        ("crates/x/tests/common/deeper.rs", "fn d() {}\n"),
    ]);
    let reach = run(dir.path(), &[LIB], &[root]);
    assert_eq!(
        reach.test_only(),
        set(&[
            root,
            "crates/x/tests/common/mod.rs",
            "crates/x/tests/common/deeper.rs"
        ])
    );
}

#[test]
fn a_file_a_test_target_and_production_both_load_is_production() {
    let root = "crates/x/tests/it.rs";
    let dir = tree(&[
        (LIB, "#[path = \"../tests/helper.rs\"]\nmod helper;\n"),
        (root, "mod helper;\n"),
        ("crates/x/tests/helper.rs", "fn h() {}\n"),
    ]);
    let reach = run(dir.path(), &[LIB], &[root]);
    assert!(reach.production.contains("crates/x/tests/helper.rs"));
    assert!(!reach.test_only().contains("crates/x/tests/helper.rs"));
}

#[test]
fn a_file_an_ordinary_module_loads_is_production() {
    let dir = tree(&[
        (LIB, "mod tests;\n"),
        ("crates/x/src/tests.rs", "fn t() {}\n"),
    ]);
    let reach = run(dir.path(), &[LIB], &[]);
    assert!(reach.test_only().is_empty());
    assert!(reach.production.contains("crates/x/src/tests.rs"));
}

#[test]
fn nested_modules_inherit_the_context() {
    let dir = tree(&[
        (
            LIB,
            "#[cfg(test)]\nmod outer {\n    mod inner;\n}\nmod live {\n    #[cfg(test)]\n    mod gated;\n    mod kept;\n}\n",
        ),
        ("crates/x/src/outer/inner.rs", "fn i() {}\n"),
        ("crates/x/src/live/gated.rs", "fn g() {}\n"),
        ("crates/x/src/live/kept.rs", "fn k() {}\n"),
    ]);
    let reach = run(dir.path(), &[LIB], &[]);
    assert_eq!(
        reach.test_only(),
        set(&["crates/x/src/outer/inner.rs", "crates/x/src/live/gated.rs"])
    );
    assert!(reach.production.contains("crates/x/src/live/kept.rs"));
}

#[test]
fn cfg_forms_decide_the_context() {
    let dir = tree(&[
        (
            LIB,
            "#[cfg(not(test))]\nmod negated;\n#[cfg(all(test, unix))]\nmod conj;\n#[cfg(any(test, unix))]\nmod either;\n",
        ),
        ("crates/x/src/negated.rs", ""),
        ("crates/x/src/conj.rs", ""),
        ("crates/x/src/either.rs", ""),
    ]);
    let reach = run(dir.path(), &[LIB], &[]);
    assert_eq!(reach.test_only(), set(&["crates/x/src/conj.rs"]));
    assert!(reach.production.contains("crates/x/src/negated.rs"));
    assert!(
        reach.production.contains("crates/x/src/either.rs"),
        "the unix branch compiles in production"
    );
}

#[test]
fn names_do_not_decide_the_context() {
    let dir = tree(&[
        (LIB, "mod tests;\n"),
        ("crates/x/src/tests.rs", ""),
        ("crates/x/src/orphan_tests.rs", ""),
    ]);
    let reach = run(dir.path(), &[LIB], &[]);
    assert!(reach.test_only().is_empty());
}

#[test]
fn unresolvable_wiring_is_refused_in_production() {
    for (text, needle) in [
        ("mod missing;\n", "no source file found"),
        (
            "#[path = concat!(\"a\", \"b\")]\nmod odd;\n",
            "not a string literal",
        ),
        (
            "#[cfg_attr(unix, path = \"x.rs\")]\nmod odd;\n",
            "`cfg_attr` sets",
        ),
        (
            "include!(concat!(env!(\"OUT_DIR\"), \"/g.rs\"));\n",
            "string literal",
        ),
        ("include!(\"../../../../../up.rs\");\n", "climbs out"),
        ("include!(\"gone.rs\");\n", "no source file found"),
    ] {
        let dir = tree(&[(LIB, text)]);
        let reach = run(dir.path(), &[LIB], &[]);
        let found = reach
            .problems
            .iter()
            .find(|problem| problem.message.contains(needle))
            .unwrap_or_else(|| panic!("{text:?}: {:#?}", reach.problems));
        assert_eq!(found.rule, "SRC-1");
        let line = if text.contains("mod odd") { 2 } else { 1 };
        assert_eq!(found.location, format!("{LIB}:{line}"), "{text:?}");
    }
}

#[test]
fn an_unparsable_production_file_is_refused() {
    let dir = tree(&[
        (LIB, "mod broken;\n"),
        ("crates/x/src/broken.rs", "fn ( {\n"),
    ]);
    let reach = run(dir.path(), &[LIB], &[]);
    assert!(
        reach
            .problems
            .iter()
            .any(|problem| problem.location == "crates/x/src/broken.rs"),
        "{:#?}",
        reach.problems
    );
}

#[test]
fn unresolvable_wiring_in_test_code_is_not_a_production_problem() {
    let dir = tree(&[(
        LIB,
        "#[cfg(test)]\nmod t {\n    include!(\"gone.rs\");\n    mod missing;\n}\n",
    )]);
    let reach = run(dir.path(), &[LIB], &[]);
    assert!(reach.problems.is_empty(), "{:#?}", reach.problems);
}

#[test]
fn a_production_path_module_outside_crates_is_followed() {
    let dir = tree(&[
        (
            LIB,
            "#[path = \"../../../shared/loaded.rs\"]\nmod loaded;\n",
        ),
        ("shared/loaded.rs", "mod nested;\n"),
        ("shared/loaded/nested.rs", "fn n() {}\n"),
    ]);
    let reach = run(dir.path(), &[LIB], &[]);
    assert!(reach.problems.is_empty(), "{:#?}", reach.problems);
    assert!(reach.production.contains("shared/loaded.rs"));
    assert!(reach.production.contains("shared/loaded/nested.rs"));
}

#[test]
fn an_include_outside_crates_is_followed_in_the_includers_context() {
    let dir = tree(&[
        (
            LIB,
            "include!(\"../../../shared/live.rs\");\n#[cfg(test)]\nmod t {\n    include!(\"../../../shared/only_tests.rs\");\n}\n",
        ),
        ("shared/live.rs", "fn live() {}\n"),
        ("shared/only_tests.rs", "fn t() {}\n"),
    ]);
    let reach = run(dir.path(), &[LIB], &[]);
    assert!(reach.problems.is_empty(), "{:#?}", reach.problems);
    assert!(reach.production.contains("shared/live.rs"));
    assert_eq!(reach.test_only(), set(&["shared/only_tests.rs"]));
}

#[test]
fn include_str_is_data_and_is_not_followed() {
    let dir = tree(&[
        (
            LIB,
            "const DATA: &str = include_str!(\"data.txt\");\nconst BYTES: &[u8] = include_bytes!(\"blob.bin\");\n",
        ),
        ("crates/x/src/data.txt", "this is not rust {{{\n"),
        ("crates/x/src/blob.bin", "\u{0}"),
    ]);
    let reach = run(dir.path(), &[LIB], &[]);
    assert!(reach.problems.is_empty(), "{:#?}", reach.problems);
    assert!(!reach.texts.contains_key("crates/x/src/data.txt"));
}

#[test]
fn a_path_into_target_is_refused_and_a_file_is_parsed_once() {
    let dir = tree(&[
        (
            LIB,
            "#[path = \"../../../target/gen.rs\"]\nmod generated;\nmod a;\nmod b;\n",
        ),
        ("target/gen.rs", "fn g() {}\n"),
        ("crates/x/src/a.rs", "#[path = \"shared.rs\"]\nmod s;\n"),
        ("crates/x/src/b.rs", "#[path = \"shared.rs\"]\nmod s;\n"),
        ("crates/x/src/shared.rs", "fn s() {}\n"),
    ]);
    let reach = run(dir.path(), &[LIB], &[]);
    assert!(
        reach
            .problems
            .iter()
            .any(|problem| problem.location == format!("{LIB}:2")
                && problem.message.contains("refused")),
        "{:#?}",
        reach.problems
    );
    assert!(!reach.texts.contains_key("target/gen.rs"));
    assert!(reach.production.contains("crates/x/src/shared.rs"));
}
