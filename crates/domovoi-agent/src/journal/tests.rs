//! Journal behaviour: durability, epochs, retention, corruption and metadata outcomes.

use std::collections::BTreeMap;
use std::path::Path;

use domovoi_config::MutationLimits;
use domovoi_proto::messages::{
    ActionState, ActionStatusQuery, AuthorizationKind, CapabilityOutcome, CapabilityResult,
    OutcomeUnknownReason, RefusalCode,
};
use jiff::Timestamp;
use tempfile::TempDir;
use uuid::Uuid;

use super::limiter::{self, Acceptance, LimitCheck};
use super::{AcceptOutcome, AcceptedRequest, JOURNAL_FILE, Journal};
use crate::fs_atomic::DurableFaults;

const T0: Timestamp = Timestamp::constant(1_700_000_000, 0);
const BOOT: &str = "boot-a";
const MONO: u64 = 1_000_000_000;

fn limits() -> MutationLimits {
    MutationLimits {
        per_hour: 30,
        per_capability: BTreeMap::new(),
    }
}

fn init(dir: &Path) -> Uuid {
    Journal::init_unlocked(dir, T0, BOOT, MONO).unwrap()
}

fn open(dir: &Path) -> Journal {
    Journal::open_unlocked(dir, BOOT, MONO, limits()).unwrap()
}

fn request<'a>(id: Uuid, contract: &'a str, mutating: bool) -> AcceptedRequest<'a> {
    AcceptedRequest {
        execution_id: id,
        contract,
        version: 1,
        target: "container:plex",
        plan_hash: "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd",
        authorization_kind: AuthorizationKind::Human,
        mutating,
    }
}

fn result() -> CapabilityResult {
    CapabilityResult {
        outcome: CapabilityOutcome::Succeeded {
            outputs: BTreeMap::new(),
        },
    }
}

fn query(epoch: Uuid, id: Uuid) -> ActionStatusQuery {
    ActionStatusQuery {
        execution_ids: vec![id],
        expected_epoch: epoch,
    }
}

fn state_of(journal: &Journal, id: Uuid) -> ActionState {
    journal
        .action_status(&query(journal.epoch(), id))
        .unwrap()
        .answers
        .remove(0)
        .state
}

#[test]
fn journal_init_creates_journal_and_epoch() {
    let dir = TempDir::new().unwrap();
    let epoch = init(dir.path());
    assert!(!epoch.is_nil());
    assert!(dir.path().join(JOURNAL_FILE).exists());
    assert!(dir.path().join(super::META_FILE).exists());
    assert_eq!(open(dir.path()).epoch(), epoch);
}

#[test]
fn journal_init_refuses_when_either_file_exists() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    assert!(Journal::init_unlocked(dir.path(), T0, BOOT, MONO).is_err());

    let only_journal = TempDir::new().unwrap();
    std::fs::write(only_journal.path().join(JOURNAL_FILE), b"x").unwrap();
    assert!(Journal::init_unlocked(only_journal.path(), T0, BOOT, MONO).is_err());
}

#[test]
fn missing_metadata_with_existing_journal_fails_closed() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    std::fs::remove_file(dir.path().join(super::META_FILE)).unwrap();
    assert!(Journal::open_unlocked(dir.path(), BOOT, MONO, limits()).is_err());
}

#[test]
fn missing_journal_with_existing_metadata_fails_closed() {
    let dir = TempDir::new().unwrap();
    let epoch = init(dir.path());
    std::fs::remove_file(dir.path().join(JOURNAL_FILE)).unwrap();
    let journal = open(dir.path());
    assert_eq!(journal.epoch(), epoch);
    assert!(journal.corruption().is_some());
    assert!(matches!(
        state_of(&journal, Uuid::now_v7()),
        ActionState::OutcomeUnknown { .. }
    ));
    assert!(matches!(
        journal.accept(
            &request(Uuid::now_v7(), "container.restart", true),
            T0,
            BOOT,
            MONO
        ),
        Ok(AcceptOutcome::Refused(_))
    ));
}

#[test]
fn empty_journal_with_existing_metadata_fails_closed() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    std::fs::File::create(dir.path().join(JOURNAL_FILE)).unwrap();
    let journal = open(dir.path());
    assert!(journal.corruption().is_some());
}

#[test]
fn accepted_is_durable_before_reply_and_spawn() {
    let dir = TempDir::new().unwrap();
    let epoch = init(dir.path());
    let id = Uuid::now_v7();
    {
        let journal = open(dir.path());
        assert_eq!(
            journal
                .accept(&request(id, "container.restart", true), T0, BOOT, MONO)
                .unwrap(),
            AcceptOutcome::Accepted
        );
    }
    let reopened = open(dir.path());
    assert_eq!(reopened.epoch(), epoch);
    assert!(matches!(
        state_of(&reopened, id),
        ActionState::OutcomeUnknown {
            reason
        } if reason.known() == Some(&OutcomeUnknownReason::NoStartRecord)
    ));
}

#[test]
fn started_is_durable_after_spawn_and_finished_before_reply() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let id = Uuid::now_v7();
    {
        let journal = open(dir.path());
        journal
            .accept(&request(id, "container.restart", true), T0, BOOT, MONO)
            .unwrap();
        journal.started(id, 4242, 100).unwrap();
        assert_eq!(state_of(&journal, id), ActionState::Running);
        journal.finished(id, result()).unwrap();
        assert!(matches!(
            state_of(&journal, id),
            ActionState::Finished { .. }
        ));
    }
    let reopened = open(dir.path());
    assert!(matches!(
        state_of(&reopened, id),
        ActionState::Finished { .. }
    ));
}

#[test]
fn duplicate_execution_id_never_spawns_twice() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let journal = open(dir.path());
    let id = Uuid::now_v7();
    let req = request(id, "container.restart", true);
    assert_eq!(
        journal.accept(&req, T0, BOOT, MONO).unwrap(),
        AcceptOutcome::Accepted
    );
    assert!(matches!(
        journal.accept(&req, T0, BOOT, MONO).unwrap(),
        AcceptOutcome::Duplicate(_)
    ));
}

#[test]
fn started_without_finished_is_never_running_after_restart() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let id = Uuid::now_v7();
    {
        let journal = open(dir.path());
        journal
            .accept(&request(id, "container.restart", true), T0, BOOT, MONO)
            .unwrap();
        journal.started(id, 999_999, 1).unwrap();
    }
    let reopened = open(dir.path());
    assert!(matches!(
        state_of(&reopened, id),
        ActionState::OutcomeUnknown {
            reason
        } if reason.known() == Some(&OutcomeUnknownReason::AgentRestarted)
    ));
}

#[test]
fn epoch_change_is_outcome_unknown() {
    let dir = TempDir::new().unwrap();
    let epoch = init(dir.path());
    let journal = open(dir.path());
    let id = Uuid::now_v7();
    journal
        .accept(&request(id, "container.restart", true), T0, BOOT, MONO)
        .unwrap();
    let answers = journal.action_status(&query(Uuid::now_v7(), id)).unwrap();
    assert_eq!(answers.epoch, epoch);
    assert!(matches!(
        answers.answers[0].state,
        ActionState::OutcomeUnknown { .. }
    ));
}

#[test]
fn not_found_with_matching_epoch_is_not_run() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let journal = open(dir.path());
    assert_eq!(state_of(&journal, Uuid::now_v7()), ActionState::NotFound);
}

#[test]
fn corrupt_metadata_fails_closed() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    std::fs::write(dir.path().join(super::META_FILE), b"{ not json").unwrap();
    assert!(Journal::open_unlocked(dir.path(), BOOT, MONO, limits()).is_err());
    assert!(dir.path().join(super::META_FILE).exists(), "never deleted");
}

#[test]
fn torn_tail_record_is_discarded_safely() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let id = Uuid::now_v7();
    {
        let journal = open(dir.path());
        journal
            .accept(&request(id, "container.restart", true), T0, BOOT, MONO)
            .unwrap();
    }
    let mut bytes = std::fs::read(dir.path().join(JOURNAL_FILE)).unwrap();
    // Declares a 10-byte payload but carries only 3 bytes.
    bytes.extend_from_slice(&[0, 0, 0, 10, 1, 2, 3]);
    std::fs::write(dir.path().join(JOURNAL_FILE), &bytes).unwrap();
    let journal = open(dir.path());
    assert!(journal.corruption().is_none());
    assert!(matches!(
        state_of(&journal, id),
        ActionState::OutcomeUnknown { .. }
    ));
}

#[test]
fn complete_final_record_checksum_mismatch_fails_closed() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let id = Uuid::now_v7();
    {
        let journal = open(dir.path());
        journal
            .accept(&request(id, "container.restart", true), T0, BOOT, MONO)
            .unwrap();
    }
    let mut bytes = std::fs::read(dir.path().join(JOURNAL_FILE)).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0x01;
    std::fs::write(dir.path().join(JOURNAL_FILE), &bytes).unwrap();
    let journal = open(dir.path());
    assert!(journal.corruption().is_some());
    assert!(matches!(
        state_of(&journal, id),
        ActionState::OutcomeUnknown { .. }
    ));
    assert!(matches!(
        journal.accept(&request(Uuid::now_v7(), "container.restart", true), T0, BOOT, MONO),
        Ok(AcceptOutcome::Refused(refused)) if refused.code.known() == Some(&RefusalCode::JournalCorrupt)
    ));
}

#[test]
fn interior_checksum_mismatch_fails_closed() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    {
        let journal = open(dir.path());
        journal
            .accept(
                &request(Uuid::now_v7(), "container.restart", true),
                T0,
                BOOT,
                MONO,
            )
            .unwrap();
    }
    let mut bytes = std::fs::read(dir.path().join(JOURNAL_FILE)).unwrap();
    // Flip a byte inside the first frame's payload.
    bytes[40] ^= 0x01;
    std::fs::write(dir.path().join(JOURNAL_FILE), &bytes).unwrap();
    let journal = open(dir.path());
    assert!(journal.corruption().is_some());
}

#[test]
fn metadata_update_preserves_pruned_flag() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    {
        let journal = open(dir.path());
        journal
            .accept(
                &request(Uuid::now_v7(), "container.restart", true),
                T0,
                BOOT,
                MONO,
            )
            .unwrap();
        journal.prune(0).unwrap();
    }
    // A new boot updates the boot state but must carry `pruned = true` forward.
    let journal = Journal::open_unlocked(dir.path(), "boot-b", 5 * MONO, limits()).unwrap();
    assert!(
        journal
            .action_status(&query(journal.epoch(), Uuid::now_v7()))
            .unwrap()
            .answers[0]
            .state
            .ne(&ActionState::NotFound)
    );
    let meta = super::Metadata::load(dir.path()).unwrap().unwrap();
    assert!(meta.pruned);
    assert_eq!(meta.boot.boot_id, "boot-b");
}

#[test]
fn boot_load_state_survives_process_restart() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let first = super::Metadata::load(dir.path()).unwrap().unwrap();
    let _ = open(dir.path());
    let after = super::Metadata::load(dir.path()).unwrap().unwrap();
    assert_eq!(after.boot, first.boot, "same boot leaves it untouched");
}

#[test]
fn boot_load_state_written_once_per_boot() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let _ = Journal::open_unlocked(dir.path(), "boot-b", 7 * MONO, limits()).unwrap();
    let first = super::Metadata::load(dir.path()).unwrap().unwrap();
    let _ = Journal::open_unlocked(dir.path(), "boot-b", 8 * MONO, limits()).unwrap();
    let second = super::Metadata::load(dir.path()).unwrap().unwrap();
    assert_eq!(first.boot, second.boot);
    assert_eq!(second.boot.first_load_mono_ns, 7 * MONO);
}

#[test]
fn not_found_requires_an_unpruned_journal() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let journal = open(dir.path());
    assert_eq!(state_of(&journal, Uuid::now_v7()), ActionState::NotFound);

    let id = Uuid::now_v7();
    journal
        .accept(&request(id, "container.restart", true), T0, BOOT, MONO)
        .unwrap();
    journal.finished(id, result()).unwrap();
    assert!(journal.prune(0).unwrap());
    assert!(matches!(
        state_of(&journal, Uuid::now_v7()),
        ActionState::OutcomeUnknown {
            reason
        } if reason.known() == Some(&OutcomeUnknownReason::JournalLost)
    ));
}

#[test]
fn crash_after_prune_marker_before_record_removal_is_safe() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let id = Uuid::now_v7();
    {
        let journal = open(dir.path());
        journal
            .accept(&request(id, "container.restart", true), T0, BOOT, MONO)
            .unwrap();
        journal.finished(id, result()).unwrap();
        // The marker is durable, then the process stops before records are rewritten.
        let meta = super::Metadata::load(dir.path()).unwrap().unwrap();
        let mut pruned = meta;
        pruned.pruned = true;
        pruned.write(dir.path(), DurableFaults::default()).unwrap();
    }
    let journal = open(dir.path());
    assert!(matches!(
        state_of(&journal, id),
        ActionState::Finished { .. }
    ));
    assert!(matches!(
        state_of(&journal, Uuid::now_v7()),
        ActionState::OutcomeUnknown { .. }
    ));
}

#[test]
fn journal_retention_never_drops_unresolved_records() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let unresolved = Uuid::now_v7();
    let resolved = Uuid::now_v7();
    let journal = open(dir.path());
    journal
        .accept(
            &request(unresolved, "container.restart", true),
            T0,
            BOOT,
            MONO,
        )
        .unwrap();
    journal
        .accept(
            &request(resolved, "container.restart", true),
            T0,
            BOOT,
            MONO,
        )
        .unwrap();
    journal.finished(resolved, result()).unwrap();
    journal.prune(0).unwrap();
    assert!(matches!(
        state_of(&journal, unresolved),
        ActionState::OutcomeUnknown {
            reason
        } if reason.known() == Some(&OutcomeUnknownReason::NoStartRecord)
    ));
    assert!(matches!(
        state_of(&journal, resolved),
        ActionState::OutcomeUnknown {
            reason
        } if reason.known() == Some(&OutcomeUnknownReason::JournalLost)
    ));
}

#[test]
fn failed_metadata_write_prevents_pruning() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let id = Uuid::now_v7();
    let journal = open(dir.path());
    journal
        .accept(&request(id, "container.restart", true), T0, BOOT, MONO)
        .unwrap();
    journal.finished(id, result()).unwrap();
    journal.inject_metadata_faults(DurableFaults {
        rename: true,
        ..DurableFaults::default()
    });
    assert!(journal.prune(0).is_err());
    assert!(matches!(
        state_of(&journal, id),
        ActionState::Finished { .. }
    ));
    let meta = super::Metadata::load(dir.path()).unwrap().unwrap();
    assert!(!meta.pruned);
}

#[test]
fn no_not_found_after_indeterminate_metadata_write() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let journal = open(dir.path());
    journal
        .accept(
            &request(Uuid::now_v7(), "container.restart", true),
            T0,
            BOOT,
            MONO,
        )
        .unwrap();
    journal.inject_metadata_faults(DurableFaults {
        dir_sync: true,
        ..DurableFaults::default()
    });
    // The marker may be visible; no records are removed and no absent id is `not_found`.
    journal.prune(0).unwrap();
    assert!(!matches!(
        state_of(&journal, Uuid::now_v7()),
        ActionState::NotFound
    ));
}

#[test]
fn journal_reset_starts_a_new_epoch() {
    let dir = TempDir::new().unwrap();
    let old = init(dir.path());
    {
        let journal = open(dir.path());
        journal
            .accept(
                &request(Uuid::now_v7(), "container.restart", true),
                T0,
                BOOT,
                MONO,
            )
            .unwrap();
    }
    let new = Journal::reset_unlocked(dir.path(), T0, BOOT, MONO).unwrap();
    assert_ne!(new, old);
    assert!(dir.path().join(format!("{JOURNAL_FILE}.{old}")).exists());
    assert!(
        dir.path()
            .join(format!("{}.{old}", super::META_FILE))
            .exists()
    );
    assert_eq!(open(dir.path()).epoch(), new);
}

#[test]
fn signal_spool_is_independent_of_the_action_journal() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    std::fs::write(dir.path().join("spool.jsonl"), b"").unwrap();
    std::fs::write(
        dir.path().join("spool.json"),
        b"{\"next_seq\":0,\"dropped\":0}",
    )
    .unwrap();
    let journal = open(dir.path());
    assert!(journal.corruption().is_none());
    assert!(dir.path().join("spool.jsonl").exists());
    assert!(dir.path().join("spool.json").exists());
}

#[test]
fn default_limit_is_thirty_per_hour() {
    let accepted: Vec<Acceptance> = (0..29)
        .map(|n| Acceptance {
            execution_id: Uuid::now_v7(),
            contract: "container.restart".to_owned(),
            boot_id: BOOT.to_owned(),
            mono_ns: MONO + n,
        })
        .collect();
    assert_eq!(
        limiter::check(&limits(), &accepted, "container.restart", MONO, BOOT, MONO),
        LimitCheck::Allowed
    );
    let full: Vec<Acceptance> = (0..30)
        .map(|n| Acceptance {
            execution_id: Uuid::now_v7(),
            contract: "container.restart".to_owned(),
            boot_id: BOOT.to_owned(),
            mono_ns: MONO + n,
        })
        .collect();
    assert!(matches!(
        limiter::check(&limits(), &full, "container.restart", MONO, BOOT, MONO),
        LimitCheck::Refused(_)
    ));
}

#[test]
fn per_capability_limit_is_stricter_than_global() {
    let mut limits = limits();
    limits
        .per_capability
        .insert("container.restart".to_owned(), 1);
    let accepted = vec![Acceptance {
        execution_id: Uuid::now_v7(),
        contract: "container.restart".to_owned(),
        boot_id: BOOT.to_owned(),
        mono_ns: MONO,
    }];
    let LimitCheck::Refused(refused) =
        limiter::check(&limits, &accepted, "container.restart", MONO, BOOT, MONO)
    else {
        panic!("expected a refusal");
    };
    assert!(matches!(
        refused.scope,
        Some(domovoi_proto::messages::RefusalScope::Capability { .. })
    ));
    assert!(refused.retry_after.is_some_and(|secs| secs >= 1));
}

#[test]
fn read_only_execution_is_not_rate_limited() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let journal = open(dir.path());
    // A read-only request never consults the limiter, whatever `per_hour` says.
    let read_only = request(Uuid::now_v7(), "system.status", false);
    assert_eq!(
        journal.accept(&read_only, T0, BOOT, MONO).unwrap(),
        AcceptOutcome::Accepted
    );
}

#[test]
fn rate_limited_request_is_not_accepted_or_started() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let journal = Journal::open_unlocked(
        dir.path(),
        BOOT,
        MONO,
        MutationLimits {
            per_hour: 1,
            per_capability: BTreeMap::new(),
        },
    )
    .unwrap();
    let first = Uuid::now_v7();
    assert_eq!(
        journal
            .accept(&request(first, "container.restart", true), T0, BOOT, MONO)
            .unwrap(),
        AcceptOutcome::Accepted
    );
    let second = Uuid::now_v7();
    assert!(matches!(
        journal
            .accept(&request(second, "container.restart", true), T0, BOOT, MONO)
            .unwrap(),
        AcceptOutcome::Refused(_)
    ));
    assert_eq!(state_of(&journal, second), ActionState::NotFound);
}
