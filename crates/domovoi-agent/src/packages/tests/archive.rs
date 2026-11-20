//! The audit pass: hostile archives, limits and the shape rules.

use domovoi_config::PackageLimits;
use domovoi_core::package::PackageFailureCode as Code;
use domovoi_testkit::{BuiltPackage, FakeClock, PackageFixture};
use jiff::SignedDuration;
use tar::EntryType;

use super::support::{SteppingClock, audit_with, code_of, name, start};
use crate::packages::archive::{audit, audit_file};
use domovoi_core::ports::Clock as _;

fn fixture() -> PackageFixture {
    PackageFixture::new("pkg-a", "1.0.0").unwrap()
}

fn limits() -> PackageLimits {
    PackageLimits::default()
}

fn expect(built: &BuiltPackage, limits: &PackageLimits, code: Code) {
    assert_eq!(code_of(audit_with(built, limits)), code);
}

fn lowered(edit: impl FnOnce(&mut PackageLimits)) -> PackageLimits {
    let mut changed = limits();
    edit(&mut changed);
    changed
}

#[test]
fn a_clean_package_passes_the_audit() {
    let built = fixture().skill("pkg_skill").build();
    let found = audit_with(&built, &limits()).unwrap();
    assert!(found.entries >= 3);
    assert!(
        String::from_utf8(found.manifest_bytes)
            .unwrap()
            .contains("pkg-a")
    );
}

#[test]
fn agent_rejects_path_escape() {
    let escapes: [&[u8]; 5] = [
        b"../x",
        b"a/../../x",
        b"/etc/passwd",
        b"pkg-a/../x",
        b"other/x",
    ];
    for path in escapes {
        let built = fixture().raw_entry(path, EntryType::Regular, b"x").build();
        expect(&built, &limits(), Code::PathEscape);
    }
    let invalid: [&[u8]; 4] = [b"pkg-a//x", b"pkg-a/a\\b", b"pkg-a/\xff", b"pkg-a/.hidden"];
    for path in invalid {
        let built = fixture().raw_entry(path, EntryType::Regular, b"x").build();
        expect(&built, &limits(), Code::PathInvalid);
    }
}

#[test]
fn agent_rejects_links_in_archive() {
    for kind in [EntryType::Symlink, EntryType::Link] {
        let built = fixture().raw_entry(b"pkg-a/l", kind, b"").build();
        expect(&built, &limits(), Code::LinkEntry);
    }
    for kind in [
        EntryType::Char,
        EntryType::Block,
        EntryType::Fifo,
        EntryType::XGlobalHeader,
    ] {
        let built = fixture().raw_entry(b"pkg-a/s", kind, b"").build();
        expect(&built, &limits(), Code::SpecialEntry);
    }
    // The tar reader itself refuses a GNU sparse header before the entry type is seen.
    let sparse = fixture()
        .raw_entry(b"pkg-a/s", EntryType::GNUSparse, b"")
        .build();
    expect(&sparse, &limits(), Code::ArchiveInvalid);
}

#[test]
fn agent_enforces_archive_limits() {
    let many = fixture()
        .file("a", b"1")
        .file("b", b"2")
        .file("c", b"3")
        .build();
    expect(&many, &lowered(|l| l.entries = 3), Code::LimitEntries);

    let big = fixture().file("a", &[1; 600]).file("b", &[2; 600]).build();
    expect(
        &big,
        &lowered(|l| l.uncompressed_bytes = 1024),
        Code::LimitUncompressed,
    );

    let wide = fixture().file("a", &[1; 600]).build();
    expect(&wide, &lowered(|l| l.file_bytes = 500), Code::LimitFileSize);

    let deep = fixture().file("a/b/c/d", b"x").build();
    expect(&deep, &lowered(|l| l.path_depth = 4), Code::LimitDepth);

    let long = fixture().file("a-very-long-file-name.txt", b"x").build();
    expect(
        &long,
        &lowered(|l| l.path_bytes = 20),
        Code::LimitPathLength,
    );
}

#[test]
fn archive_over_the_time_limit_is_refused() {
    let built = fixture().file("a", b"1").file("b", b"2").build();
    let clock = SteppingClock::new(SignedDuration::from_secs(1));
    let limits = lowered(|l| l.extraction_time = domovoi_config::Duration::from_secs(2).unwrap());
    let result = audit(
        built.archive.as_slice(),
        &name(),
        &limits,
        &clock,
        &built.archive_sha256,
    );
    assert_eq!(code_of(result), Code::LimitTime);
}

#[test]
fn decompression_bomb_stops_at_the_cap() {
    let built = fixture().trailing(&vec![0_u8; 64 * 1024 * 1024]).build();
    assert!(built.archive.len() < 1024 * 1024);
    expect(
        &built,
        &lowered(|l| l.uncompressed_bytes = 1024 * 1024),
        Code::LimitUncompressed,
    );
}

#[test]
fn duplicate_and_case_colliding_paths_are_rejected() {
    let same = fixture().file("a", b"1").file("a", b"2").build();
    expect(&same, &limits(), Code::DuplicatePath);
    let cased = fixture().file("Readme", b"1").file("readme", b"2").build();
    expect(&cased, &limits(), Code::DuplicatePath);
}

#[test]
fn file_directory_clash_is_rejected() {
    let under = fixture().file("a", b"1").file("a/b", b"2").build();
    expect(&under, &limits(), Code::DuplicatePath);
    let over = fixture().file("a/b", b"2").file("a", b"1").build();
    expect(&over, &limits(), Code::DuplicatePath);
}

#[test]
fn data_after_archive_end_is_rejected() {
    let built = fixture().trailing(b"junk after the end").build();
    expect(&built, &limits(), Code::ArchiveInvalid);
}

#[test]
fn not_zstd_is_archive_invalid() {
    let built = fixture().build();
    let clock = FakeClock::new(start());
    let result = audit(
        b"this is not zstd at all".as_slice(),
        &name(),
        &limits(),
        &clock,
        &built.archive_sha256,
    );
    assert_eq!(code_of(result), Code::ArchiveInvalid);
}

#[test]
fn missing_or_oversize_package_toml_is_manifest_invalid() {
    let missing = fixture().without_manifest().file("a", b"1").build();
    expect(&missing, &limits(), Code::ManifestInvalid);
    let huge = fixture().manifest_bytes(&vec![b'#'; 70 * 1024]).build();
    expect(&huge, &limits(), Code::ManifestInvalid);
}

#[test]
fn archive_with_another_digest_is_a_digest_mismatch() {
    let built = fixture().file("a", b"1").build();
    let other = fixture().file("a", b"2").build();
    let clock = FakeClock::new(start());
    let result = audit(
        built.archive.as_slice(),
        &name(),
        &limits(),
        &clock,
        &other.archive_sha256,
    );
    assert_eq!(code_of(result), Code::DigestMismatch);
}

#[test]
fn audit_pass_writes_nothing() {
    let root = tempfile::tempdir().unwrap();
    let listing = |dir: &std::path::Path| {
        let mut names: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        names.sort();
        names
    };
    let before = listing(root.path());
    for built in [
        fixture()
            .raw_entry(b"../x", EntryType::Regular, b"x")
            .build(),
        fixture()
            .raw_entry(b"pkg-a/l", EntryType::Symlink, b"")
            .build(),
        fixture().skill("pkg_skill").build(),
    ] {
        let _ = audit_with(&built, &limits());
    }
    assert_eq!(listing(root.path()), before);
}

#[test]
fn audit_file_refuses_a_file_over_the_compressed_cap() {
    let root = tempfile::tempdir().unwrap();
    let built = fixture().file("a", &[7; 4096]).build();
    let path = root.path().join("pkg.tar.zst");
    std::fs::write(&path, &built.archive).unwrap();
    let clock = FakeClock::new(start());
    let result = audit_file(
        &path,
        &name(),
        &lowered(|l| l.compressed_bytes = 16),
        &clock,
        &built.archive_sha256,
    );
    assert_eq!(code_of(result), Code::TooLarge);
}

#[test]
fn excess_padding_after_the_end_marker_is_refused_below_the_global_cap() {
    let built = fixture().trailing(&vec![0_u8; 1024 * 1024]).build();
    expect(&built, &limits(), Code::LimitUncompressed);
}

#[test]
fn ordinary_record_padding_after_the_end_marker_is_accepted() {
    let built = fixture().trailing(&vec![0_u8; 8 * 1024]).build();
    audit_with(&built, &limits()).unwrap();
}

#[test]
fn deadline_is_enforced_while_reading_the_trailing_padding() {
    let plain = fixture().build();
    let padded = fixture().trailing(&vec![0_u8; 12 * 1024]).build();
    let clock = SteppingClock::new(SignedDuration::from_secs(1));
    audit(
        plain.archive.as_slice(),
        &name(),
        &limits(),
        &clock,
        &plain.archive_sha256,
    )
    .unwrap();
    // The clock now reads one second past the last check of the plain archive.
    let seconds = clock.now().duration_since(start()).as_secs();
    let limits = lowered(|l| {
        l.extraction_time = domovoi_config::Duration::from_secs(seconds).unwrap();
    });
    let clock = SteppingClock::new(SignedDuration::from_secs(1));
    let result = audit(
        padded.archive.as_slice(),
        &name(),
        &limits,
        &clock,
        &padded.archive_sha256,
    );
    assert_eq!(code_of(result), Code::LimitTime);
}
