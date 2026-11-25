use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;

use domovoi_core::attest::{Hash32, ZERO_HASH};
use domovoi_core::helper::UnsignedToken;
use domovoi_helper::attest::state::StateFile;
use domovoi_helper::attest::{AttestOutcome, public_key};

use crate::world::{BOOT_A, World, boot, reason};

fn unsigned(outcome: AttestOutcome) -> UnsignedToken {
    match outcome {
        AttestOutcome::Unsigned(token) => token,
        AttestOutcome::Signed(_) => panic!("expected an unsigned token"),
    }
}

#[tokio::test]
async fn generation_state_missing_with_key_fails_closed() {
    let world = World::new();
    world.init();
    std::fs::remove_file(world.path("state")).expect("lose the state");
    for n in 1..=2 {
        assert_eq!(reason(&world.signed(n).await), "state_missing");
    }
    assert!(
        !world.path("state").exists(),
        "the state is never recreated by an attestation"
    );
}

#[tokio::test]
async fn generation_state_corrupt_with_key_fails_closed() {
    let world = World::new();
    world.init();
    let good = std::fs::read(world.path("state")).expect("state");
    for damaged in [
        good[..good.len() - 3].to_vec(),
        b"format=1\n".to_vec(),
        Vec::new(),
        good.iter().map(|b| b.to_ascii_uppercase()).collect(),
    ] {
        std::fs::write(world.path("state"), &damaged).expect("damage");
        assert_eq!(reason(&world.signed(1).await), "state_corrupt");
        assert_eq!(
            std::fs::read(world.path("state")).expect("state"),
            damaged,
            "never repaired"
        );
    }
}

#[tokio::test]
async fn generation_state_key_mismatch_fails_closed() {
    let world = World::new();
    world.init();
    let foreign = StateFile::fresh(Hash32::of(b"another key").into(), boot(BOOT_A));
    std::fs::write(world.path("state"), foreign.render()).expect("replace");
    assert_eq!(reason(&world.signed(1).await), "state_key_mismatch");
    assert_eq!(
        std::fs::read(world.path("state")).expect("state"),
        foreign.render()
    );
}

#[tokio::test]
async fn helper_state_recovery_requires_explicit_local_action() {
    // Neither a missing nor a corrupt state is ever reset by serving requests.
    let world = World::new();
    world.init();
    std::fs::remove_file(world.path("state")).expect("lose the state");
    for n in 1..=3 {
        assert_eq!(reason(&world.signed(n).await), "state_missing");
    }
    let state = world.dir().join("state");
    assert!(!state.exists());
    assert!(public_key(&world.env()).is_ok(), "the key is still served");
}

#[tokio::test]
async fn unsigned_tokens_never_carry_a_signature() {
    let mut seen = BTreeSet::new();

    // Neither key nor state: the directory and lock only.
    let bare = World::new();
    domovoi_helper::state::init(crate::world::layout(), bare.dir()).expect("init dir");
    seen.insert(unsigned(bare.attest(1).await));

    // A state without a key. The helper never makes a key by itself.
    let no_key = World::new();
    no_key.init();
    std::fs::remove_file(no_key.path("attest.key")).expect("lose the key");
    seen.insert(unsigned(no_key.attest(1).await));
    assert!(!no_key.path("attest.key").exists(), "no key is generated");

    // A key file that is not a 32-byte seed.
    for damage in [&b"short"[..], &[7_u8; 33][..], &b""[..]] {
        let corrupt = World::new();
        corrupt.init();
        std::fs::write(corrupt.path("attest.key"), damage).expect("damage");
        seen.insert(unsigned(corrupt.attest(1).await));
    }

    // Wrong modes, from the scripted facts of the start check.
    let wrong_dir = World::new();
    wrong_dir.init();
    let mut loose = wrong_dir.sharing();
    loose.text.dir_mode = 0o755;
    seen.insert(unsigned(loose.attest(1).await));
    let mut loose_key = wrong_dir.sharing();
    loose_key.text.key_mode = Some(0o644);
    seen.insert(unsigned(loose_key.attest(1).await));

    // A lock that cannot be opened.
    let no_lock = World::new();
    no_lock.init();
    std::fs::remove_file(no_lock.path("state.lock")).expect("remove the lock");
    seen.insert(unsigned(no_lock.attest(1).await));

    assert_eq!(
        seen,
        UnsignedToken::ALL.into_iter().collect::<BTreeSet<_>>()
    );
}

#[tokio::test]
async fn lock_open_error_has_its_own_token() {
    let world = World::new();
    world.init();
    std::fs::remove_file(world.path("state.lock")).expect("remove the lock");
    assert_eq!(
        unsigned(world.attest(1).await),
        UnsignedToken::LockUnavailable
    );
    // A directory in place of the lock file is not a lock either.
    std::fs::create_dir(world.path("state.lock")).expect("dir");
    assert_eq!(
        unsigned(world.attest(2).await),
        UnsignedToken::LockUnavailable
    );
    assert_eq!(
        public_key(&world.env()),
        Err(UnsignedToken::LockUnavailable)
    );
}

// covers: helper_state_permissions
#[tokio::test]
async fn helper_fails_closed_on_wrong_directory_mode() {
    let world = World::new();
    world.init();
    for mode in [0o755, 0o750, 0o701, 0o777] {
        let mut loose = world.sharing();
        loose.text.dir_mode = mode;
        assert_eq!(
            unsigned(loose.attest(1).await),
            UnsignedToken::HelperStatePermissions,
            "{mode:o}"
        );
        assert_eq!(
            public_key(&loose.env()),
            Err(UnsignedToken::HelperStatePermissions)
        );
    }
    // The real directory is untouched, and a correct mode serves.
    assert_eq!(
        std::fs::metadata(world.dir())
            .expect("dir")
            .permissions()
            .mode()
            & 0o7777,
        0o700
    );
    assert!(matches!(world.attest(2).await, AttestOutcome::Signed(_)));
}

#[tokio::test]
async fn helper_fails_closed_when_safety_state_unknown() {
    // The route state cannot be read at all: the helper signs why instead of guessing.
    let world = World::new();
    world.init();
    let mut broken = world.sharing();
    broken.exec = std::sync::Arc::new(domovoi_testkit::FakeProcessExecutor::new());
    assert_eq!(reason(&broken.signed(1).await), "rules_unreadable");
    let state =
        StateFile::parse(&std::fs::read(world.path("state")).expect("state")).expect("state");
    assert_eq!((state.generation, state.config_hash), (0, ZERO_HASH));
}
