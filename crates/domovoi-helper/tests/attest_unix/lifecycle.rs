use domovoi_core::attest::ZERO_HASH;
use domovoi_core::host::KeyFingerprint;
use domovoi_helper::attest::keys::{fingerprint_groups, load_key};
use domovoi_helper::attest::lifecycle::{
    LifecycleError, fingerprint, init_attestation, recover, rotate_key,
};
use domovoi_helper::attest::state::StateFile;

use crate::world::{BOOT_A, BOOT_B, World, boot, generation, layout, mode_of};

fn read_state(world: &World) -> StateFile {
    StateFile::parse(&std::fs::read(world.path("state")).expect("state")).expect("valid state")
}

fn seed(world: &World) -> Vec<u8> {
    std::fs::read(world.path("attest.key")).expect("key")
}

/// Every regular file under `dir`, with its bytes.
fn tree(dir: &std::path::Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            out.extend(tree(&path));
        } else {
            let bytes = std::fs::read(&path).expect("read");
            out.push((path, bytes));
        }
    }
    out
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn init_writes_generation_zero_and_zero_hashes() {
    let world = World::new();
    let info = world.init();
    let state = read_state(&world);
    assert_eq!(state.generation, 0);
    assert_eq!(state.config_hash, ZERO_HASH);
    assert_eq!(state.route_state_hash, ZERO_HASH);
    assert_eq!(state.boot_id, boot(BOOT_A));
    assert_eq!(state.key_fingerprint, info.fingerprint);
    assert_eq!(seed(&world).len(), 32, "the key file is exactly the seed");
    for (name, mode) in [
        ("attest.key", 0o600),
        ("state", 0o600),
        ("state.lock", 0o600),
    ] {
        assert_eq!(world.mode(name), mode, "{name}");
    }
    assert_eq!(mode_of(world.dir()), 0o700);
    assert!(!world.path("retired").exists());
}

#[test]
fn init_refuses_existing_key_or_state() {
    let world = World::new();
    world.init();
    let again = init_attestation(layout(), world.dir(), &boot(BOOT_A));
    assert!(matches!(again, Err(LifecycleError::AlreadyInitialised)));

    let key_before = seed(&world);
    std::fs::remove_file(world.path("state")).expect("remove state");
    assert!(
        matches!(
            init_attestation(layout(), world.dir(), &boot(BOOT_A)),
            Err(LifecycleError::AlreadyInitialised)
        ),
        "a key alone also refuses"
    );
    assert_eq!(seed(&world), key_before, "a refused init changes nothing");

    std::fs::remove_file(world.path("attest.key")).expect("remove key");
    std::fs::write(world.path("state"), b"anything").expect("write state");
    assert!(
        matches!(
            init_attestation(layout(), world.dir(), &boot(BOOT_A)),
            Err(LifecycleError::AlreadyInitialised)
        ),
        "a state alone also refuses"
    );
}

#[tokio::test]
async fn first_attestation_is_generation_one_after_init() {
    let world = World::new();
    world.init();
    assert_eq!(generation(&world.signed(1).await), 1);
    assert_eq!(read_state(&world).generation, 1);
    assert_eq!(generation(&world.signed(2).await), 1, "nothing changed");
}

#[test]
fn rotate_key_deletes_old_private_key() {
    let world = World::new();
    let old = world.init();
    let old_seed = seed(&world);
    let replaced =
        rotate_key(layout(), world.dir(), &boot(BOOT_B), world.clock_now()).expect("rotate");
    assert_ne!(replaced.new.fingerprint, old.fingerprint);
    assert_eq!(
        replaced.old.as_ref().map(|k| k.fingerprint),
        Some(old.fingerprint)
    );
    assert_ne!(seed(&world), old_seed);
    for (path, bytes) in tree(world.dir()) {
        assert!(
            !contains(&bytes, &old_seed),
            "{} holds the old private key",
            path.display()
        );
    }
    let state = read_state(&world);
    assert_eq!(state.key_fingerprint, replaced.new.fingerprint);
    assert_eq!((state.generation, state.config_hash), (0, ZERO_HASH));
    assert_eq!(state.boot_id, boot(BOOT_B));
}

#[test]
fn retired_directory_keeps_public_key_and_state_only() {
    let world = World::new();
    let old = world.init();
    let old_state = std::fs::read(world.path("state")).expect("state");
    let old_seed = seed(&world);
    let replaced =
        rotate_key(layout(), world.dir(), &boot(BOOT_A), world.clock_now()).expect("rotate");
    assert_eq!(
        replaced.retired,
        world.path("retired").join("20260701T120000Z")
    );
    let mut names: Vec<String> = std::fs::read_dir(&replaced.retired)
        .expect("retired")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["fingerprint", "public.key", "state"]);
    assert_eq!(
        std::fs::read(replaced.retired.join("state")).expect("copy"),
        old_state
    );
    let public = std::fs::read_to_string(replaced.retired.join("public.key")).expect("public");
    assert_eq!(public.trim().len(), 64);
    let print = std::fs::read_to_string(replaced.retired.join("fingerprint")).expect("print");
    assert!(print.starts_with(&fingerprint_groups(&old.fingerprint)));
    for (path, bytes) in tree(&world.path("retired")) {
        assert!(
            !contains(&bytes, &old_seed),
            "{} holds the seed",
            path.display()
        );
        assert_eq!(mode_of(&path), 0o600);
    }
    assert_eq!(mode_of(&replaced.retired), 0o700);
    // A second rotation in the same second does not overwrite the retired record.
    assert!(rotate_key(layout(), world.dir(), &boot(BOOT_A), world.clock_now()).is_err());
}

#[test]
fn rotate_key_needs_a_usable_key_and_a_state() {
    let world = World::new();
    world.init();
    std::fs::remove_file(world.path("state")).expect("remove");
    assert!(matches!(
        rotate_key(layout(), world.dir(), &boot(BOOT_A), world.clock_now()),
        Err(LifecycleError::NotRotatable)
    ));
}

#[tokio::test]
async fn recover_always_creates_new_key() {
    let world = World::new();
    let old = world.init();
    assert_eq!(generation(&world.signed(1).await), 1);
    std::fs::remove_file(world.path("state")).expect("lose the state");
    let replaced =
        recover(layout(), world.dir(), &boot(BOOT_A), world.clock_now()).expect("recover");
    assert_ne!(
        replaced.new.fingerprint, old.fingerprint,
        "continuity cannot be proven"
    );
    assert_eq!(
        generation(&world.signed(2).await),
        1,
        "the sequence restarts under the new key"
    );

    // A damaged key is recovered the same way, and nothing readable of it is copied.
    std::fs::write(world.path("attest.key"), b"short").expect("damage the key");
    let again = recover(
        layout(),
        world.dir(),
        &boot(BOOT_A),
        world.clock_now_plus(5),
    )
    .expect("recover");
    assert_eq!(again.old, None);
    assert_ne!(again.new.fingerprint, replaced.new.fingerprint);
    assert!(load_key(world.dir()).is_ok());

    let empty = World::new();
    std::fs::create_dir(empty.dir()).expect("dir");
    assert!(matches!(
        recover(layout(), empty.dir(), &boot(BOOT_A), empty.clock_now()),
        Err(LifecycleError::NothingToRecover)
    ));
}

#[tokio::test]
async fn key_rotation_invalidates_previous_pin() {
    let world = World::new();
    world.init();
    let before = world.signed(1).await;
    rotate_key(layout(), world.dir(), &boot(BOOT_A), world.clock_now()).expect("rotate");
    let after = world.signed(2).await;
    assert_ne!(
        before.public_key, after.public_key,
        "a rotated helper presents a new key"
    );
    assert_eq!(generation(&after), 1, "the sequence is per key");
    let old = ed25519_dalek::VerifyingKey::from_bytes(&before.public_key).expect("key");
    let sig = ed25519_dalek::Signature::from_bytes(&after.signature);
    assert!(
        old.verify_strict(&domovoi_core::attest::signing_bytes(&after.report), &sig)
            .is_err(),
        "the previous pin does not verify the new key's reports"
    );
}

#[test]
fn fingerprint_command_matches_hub_computation() {
    let world = World::new();
    let info = world.init();
    let shown = fingerprint(world.dir()).expect("fingerprint");
    assert_eq!(shown, info);
    let key = load_key(world.dir()).expect("key");
    assert_eq!(
        shown.fingerprint,
        KeyFingerprint::of_public_key(&key.public_key())
    );
    assert_eq!(shown.key_id, shown.fingerprint.to_string()[..16]);
    let groups = shown.grouped();
    assert_eq!(groups.split(' ').count(), 16);
    assert!(groups.split(' ').all(|g| g.len() == 4));
    assert_eq!(groups.replace(' ', ""), shown.fingerprint.to_string());
    std::fs::write(world.path("attest.key"), b"x").expect("damage");
    assert!(matches!(
        fingerprint(world.dir()),
        Err(LifecycleError::NoKey)
    ));
}
