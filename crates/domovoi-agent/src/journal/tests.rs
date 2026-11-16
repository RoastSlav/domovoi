//! Journal behaviour: durability, epochs, retention, corruption and metadata outcomes.

use std::collections::BTreeMap;
use std::path::Path;

use domovoi_config::MutationLimits;
use domovoi_proto::messages::{
    ActionState, ActionStatusQuery, AuthorizationKind, CapabilityOutcome, CapabilityResult,
    OutcomeUnknownReason, RefusalCode,
};
#[cfg(target_os = "linux")]
use jiff::SignedDuration;
use jiff::Timestamp;
use sha2::Digest as _;
use tempfile::TempDir;
use uuid::Uuid;

use super::limiter::{self, Acceptance, LimitCheck};
use super::{AcceptOutcome, AcceptedRequest, AppendFault, JOURNAL_FILE, Journal, Record};
#[cfg(target_os = "linux")]
use crate::error::JournalError;
use crate::fs_atomic::DurableFaults;

const T0: Timestamp = Timestamp::constant(1_700_000_000, 0);
const BOOT: &str = "018f0000-0000-7000-8000-000000000001";
const BOOT_B: &str = "018f0000-0000-7000-8000-000000000002";
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

/// Accepts, starts and finishes one execution.
fn finished_execution(journal: &Journal, id: Uuid) {
    journal
        .accept(&request(id, "container.restart", true), T0, BOOT, MONO)
        .unwrap();
    journal.started(id, 1, 1).unwrap();
    journal.finished(id, result()).unwrap();
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
    assert_eq!(committed_len(dir.path()), journal_len(dir.path()));
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

    let only_boundary = TempDir::new().unwrap();
    super::commit::write(only_boundary.path(), 0, DurableFaults::default()).unwrap();
    assert!(Journal::init_unlocked(only_boundary.path(), T0, BOOT, MONO).is_err());
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
        finished_execution(&journal, Uuid::now_v7());
        journal
            .prune(0, MONO + super::limiter::WINDOW_NS + 1, BOOT)
            .unwrap();
    }
    // A new boot updates the boot state but must carry `pruned = true` forward.
    let journal = Journal::open_unlocked(dir.path(), BOOT_B, 5 * MONO, limits()).unwrap();
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
    assert_eq!(meta.boot.boot_id, BOOT_B);
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
    let _ = Journal::open_unlocked(dir.path(), BOOT_B, 7 * MONO, limits()).unwrap();
    let first = super::Metadata::load(dir.path()).unwrap().unwrap();
    let _ = Journal::open_unlocked(dir.path(), BOOT_B, 8 * MONO, limits()).unwrap();
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

    finished_execution(&journal, Uuid::now_v7());
    assert!(
        journal
            .prune(0, MONO + super::limiter::WINDOW_NS + 1, BOOT)
            .unwrap()
    );
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
        finished_execution(&journal, id);
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
    finished_execution(&journal, resolved);
    journal
        .prune(0, MONO + super::limiter::WINDOW_NS + 1, BOOT)
        .unwrap();
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
    finished_execution(&journal, id);
    journal.inject_metadata_faults(DurableFaults {
        rename: true,
        ..DurableFaults::default()
    });
    assert!(
        journal
            .prune(0, MONO + super::limiter::WINDOW_NS + 1, BOOT)
            .is_err()
    );
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
    finished_execution(&journal, Uuid::now_v7());
    journal.inject_metadata_faults(DurableFaults {
        dir_sync: true,
        ..DurableFaults::default()
    });
    // The marker may be visible; no records are removed and no absent id is `not_found`.
    journal
        .prune(0, MONO + super::limiter::WINDOW_NS + 1, BOOT)
        .unwrap();
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

// ---- Startup and ownership -------------------------------------------------

#[test]
fn missing_metadata_and_missing_journal_reports_init() {
    let dir = TempDir::new().unwrap();
    let err = Journal::open_unlocked(dir.path(), BOOT, MONO, limits()).unwrap_err();
    assert!(err.to_string().contains("journal init"), "{err}");
}

#[test]
fn missing_metadata_with_existing_journal_reports_recovery() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    std::fs::remove_file(dir.path().join(super::META_FILE)).unwrap();
    let err = Journal::open_unlocked(dir.path(), BOOT, MONO, limits()).unwrap_err();
    assert!(err.to_string().contains("journal reset"), "{err}");
}

#[test]
fn invalid_metadata_never_creates_a_fresh_journal() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let journal_before = std::fs::read(dir.path().join(JOURNAL_FILE)).unwrap();
    std::fs::write(dir.path().join(super::META_FILE), b"{ not json").unwrap();
    assert!(Journal::open_unlocked(dir.path(), BOOT, MONO, limits()).is_err());
    assert_eq!(
        std::fs::read(dir.path().join(JOURNAL_FILE)).unwrap(),
        journal_before
    );
    assert!(
        Journal::open_unlocked(dir.path(), BOOT, MONO, limits()).is_err(),
        "a second attempt must not repair or replace the metadata"
    );
}

#[test]
fn corrupt_journal_never_reports_not_found() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    std::fs::File::create(dir.path().join(JOURNAL_FILE)).unwrap();
    let journal = open(dir.path());
    assert!(matches!(
        state_of(&journal, Uuid::now_v7()),
        ActionState::OutcomeUnknown {
            reason
        } if reason.known() == Some(&OutcomeUnknownReason::JournalLost)
    ));
}

#[cfg(target_os = "linux")]
#[test]
fn second_open_on_the_same_lock_is_refused() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let first = Journal::open(dir.path(), BOOT, MONO, limits()).unwrap();
    assert!(matches!(
        Journal::open(dir.path(), BOOT, MONO, limits()),
        Err(JournalError::Locked(_))
    ));
    drop(first);
    assert!(Journal::open(dir.path(), BOOT, MONO, limits()).is_ok());
}

// ---- Append failures -------------------------------------------------------

#[test]
fn partial_acceptance_write_does_not_permit_a_spawn() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let journal = open(dir.path());
    let id = Uuid::now_v7();
    journal.inject_append_fault(AppendFault::Partial);
    assert!(
        journal
            .accept(&request(id, "container.restart", true), T0, BOOT, MONO)
            .is_err()
    );
    assert_eq!(state_of(&journal, id), ActionState::NotFound);
    assert!(journal.corruption().is_none(), "a torn tail is recoverable");
}

#[test]
fn failed_acceptance_sync_never_acknowledges() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let id = Uuid::now_v7();
    {
        let journal = open(dir.path());
        // Both the first sync and the recovery sync fail: the frame is uncommitted and the
        // repair returns to the durable boundary.
        journal.inject_append_fault(AppendFault::RecoverySync);
        assert!(
            journal
                .accept(&request(id, "container.restart", true), T0, BOOT, MONO)
                .is_err()
        );
        assert!(
            journal.corruption().is_none(),
            "a synced repair leaves a fully known state"
        );
        assert_eq!(state_of(&journal, id), ActionState::NotFound);
    }
    // The unconfirmed frame was truncated; the previous journal state is intact and no
    // spawned execution exists.
    let reopened = open(dir.path());
    assert_eq!(state_of(&reopened, id), ActionState::NotFound);
}

#[test]
fn successful_recovery_sync_confirms_acceptance() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let id = Uuid::now_v7();
    let journal = open(dir.path());
    journal.inject_append_fault(AppendFault::Sync);
    assert_eq!(
        journal
            .accept(&request(id, "container.restart", true), T0, BOOT, MONO)
            .unwrap(),
        AcceptOutcome::Accepted,
        "the recovery sync made the acceptance durable"
    );
    assert!(matches!(
        state_of(&journal, id),
        ActionState::OutcomeUnknown {
            reason
        } if reason.known() == Some(&OutcomeUnknownReason::NoStartRecord)
    ));
    assert!(journal.corruption().is_none());
}

#[test]
fn uncertain_acceptance_retry_cannot_spawn_twice() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let journal = open(dir.path());
    let id = Uuid::now_v7();
    let req = request(id, "container.restart", true);
    journal.inject_append_fault(AppendFault::Uncertain);
    assert!(journal.accept(&req, T0, BOOT, MONO).is_err());
    // The frame may be visible; reconciling adopted it, so a retry is a duplicate.
    assert!(matches!(
        journal.accept(&req, T0, BOOT, MONO).unwrap(),
        AcceptOutcome::Duplicate(_)
    ));
}

#[test]
fn journal_cannot_append_behind_a_partial_frame() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let journal = open(dir.path());
    journal.inject_append_fault(AppendFault::Partial);
    assert!(
        journal
            .accept(
                &request(Uuid::now_v7(), "container.restart", true),
                T0,
                BOOT,
                MONO
            )
            .is_err()
    );
    let id = Uuid::now_v7();
    assert_eq!(
        journal
            .accept(&request(id, "container.restart", true), T0, BOOT, MONO)
            .unwrap(),
        AcceptOutcome::Accepted
    );
    let reopened = open(dir.path());
    assert_eq!(reopened.epoch(), journal.epoch());
    assert!(matches!(
        state_of(&reopened, id),
        ActionState::OutcomeUnknown { .. }
    ));
}

#[test]
fn failed_started_append_does_not_leave_running() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let journal = open(dir.path());
    let id = Uuid::now_v7();
    journal
        .accept(&request(id, "container.restart", true), T0, BOOT, MONO)
        .unwrap();
    journal.inject_append_fault(AppendFault::Open);
    assert!(journal.started(id, 1, 1).is_err());
    assert!(!matches!(state_of(&journal, id), ActionState::Running));
    let reopened = open(dir.path());
    assert!(!matches!(state_of(&reopened, id), ActionState::Running));
}

#[test]
fn failed_finished_append_never_reports_durable_success() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let journal = open(dir.path());
    let id = Uuid::now_v7();
    journal
        .accept(&request(id, "container.restart", true), T0, BOOT, MONO)
        .unwrap();
    journal.started(id, 1, 1).unwrap();
    journal.inject_append_fault(AppendFault::RecoverySync);
    assert!(journal.finished(id, result()).is_err());
    assert!(
        !matches!(state_of(&journal, id), ActionState::Finished { .. }),
        "an unconfirmed finish is never authoritative"
    );
}

#[test]
fn successful_recovery_sync_makes_finished_durable() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let journal = open(dir.path());
    let id = Uuid::now_v7();
    journal
        .accept(&request(id, "container.restart", true), T0, BOOT, MONO)
        .unwrap();
    journal.started(id, 1, 1).unwrap();
    journal.inject_append_fault(AppendFault::Sync);
    journal.finished(id, result()).unwrap();
    assert!(matches!(
        state_of(&journal, id),
        ActionState::Finished { .. }
    ));
    assert!(journal.corruption().is_none());
}

#[test]
fn unconfirmed_complete_frame_truncation_failure_fences_and_refuses_more_appends() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let journal = open(dir.path());
    journal.inject_append_fault(AppendFault::RecoverySyncTruncateFail);
    assert!(
        journal
            .accept(
                &request(Uuid::now_v7(), "container.restart", true),
                T0,
                BOOT,
                MONO
            )
            .is_err()
    );
    assert!(journal.corruption().is_some());
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

// ---- Lifecycle validation --------------------------------------------------

fn created(epoch: Uuid) -> Record {
    Record::JournalCreated {
        format: Record::FORMAT,
        epoch,
        wall_time: T0,
        boot_id: BOOT.to_owned(),
        mono_ns: MONO,
    }
}

fn accepted(id: Uuid) -> Record {
    Record::Accepted {
        format: Record::FORMAT,
        execution_id: id,
        contract: "container.restart".to_owned(),
        version: 1,
        target: "container:plex".to_owned(),
        plan_hash: "cd".repeat(32),
        authorization_kind: AuthorizationKind::Human,
        wall_time: T0,
        boot_id: BOOT.to_owned(),
        mono_ns: MONO,
        mutating: true,
    }
}

fn started_rec(id: Uuid) -> Record {
    Record::Started {
        format: Record::FORMAT,
        execution_id: id,
        pid: 1,
        proc_start_time: 1,
        pgid: 1,
        session: 1,
    }
}

fn finished_rec(id: Uuid) -> Record {
    Record::Finished {
        format: Record::FORMAT,
        execution_id: id,
        result: result(),
    }
}

fn abandoned_rec(id: Uuid) -> Record {
    Record::Abandoned {
        format: Record::FORMAT,
        execution_id: id,
        pid: 1,
        reason: "test".to_owned(),
    }
}

fn write_records(dir: &Path, records: &[Record]) {
    let mut bytes = Vec::new();
    for record in records {
        bytes.extend_from_slice(&super::record::frame(record).unwrap());
    }
    std::fs::write(dir.join(JOURNAL_FILE), &bytes).unwrap();
    // Every journal file created outside `init` still needs a commit boundary that names
    // its complete contents.
    super::commit::write(dir, bytes.len() as u64, DurableFaults::default()).unwrap();
}

fn committed_len(dir: &Path) -> u64 {
    super::commit::load(dir).unwrap()
}

fn journal_len(dir: &Path) -> u64 {
    std::fs::metadata(dir.join(JOURNAL_FILE)).unwrap().len()
}

fn assert_corrupt(dir: &Path) {
    let journal = open(dir);
    assert!(journal.corruption().is_some(), "expected a fenced journal");
}

#[test]
fn rejects_started_without_accepted() {
    let dir = TempDir::new().unwrap();
    let epoch = init(dir.path());
    write_records(dir.path(), &[created(epoch), started_rec(Uuid::now_v7())]);
    assert_corrupt(dir.path());
}

#[test]
fn rejects_duplicate_accepted() {
    let dir = TempDir::new().unwrap();
    let epoch = init(dir.path());
    let id = Uuid::now_v7();
    write_records(dir.path(), &[created(epoch), accepted(id), accepted(id)]);
    assert_corrupt(dir.path());
}

#[test]
fn rejects_finished_without_started() {
    let dir = TempDir::new().unwrap();
    let epoch = init(dir.path());
    let id = Uuid::now_v7();
    write_records(
        dir.path(),
        &[created(epoch), accepted(id), finished_rec(id)],
    );
    assert_corrupt(dir.path());
}

#[test]
fn rejects_duplicate_started() {
    let dir = TempDir::new().unwrap();
    let epoch = init(dir.path());
    let id = Uuid::now_v7();
    write_records(
        dir.path(),
        &[
            created(epoch),
            accepted(id),
            started_rec(id),
            started_rec(id),
        ],
    );
    assert_corrupt(dir.path());
}

#[test]
fn rejects_abandoned_without_started() {
    let dir = TempDir::new().unwrap();
    let epoch = init(dir.path());
    let id = Uuid::now_v7();
    write_records(
        dir.path(),
        &[created(epoch), accepted(id), abandoned_rec(id)],
    );
    assert_corrupt(dir.path());
}

#[test]
fn rejects_abandoned_after_finished() {
    let dir = TempDir::new().unwrap();
    let epoch = init(dir.path());
    let id = Uuid::now_v7();
    write_records(
        dir.path(),
        &[
            created(epoch),
            accepted(id),
            started_rec(id),
            finished_rec(id),
            abandoned_rec(id),
        ],
    );
    assert_corrupt(dir.path());
}

#[test]
fn rejects_duplicate_finished() {
    let dir = TempDir::new().unwrap();
    let epoch = init(dir.path());
    let id = Uuid::now_v7();
    write_records(
        dir.path(),
        &[
            created(epoch),
            accepted(id),
            started_rec(id),
            finished_rec(id),
            finished_rec(id),
        ],
    );
    assert_corrupt(dir.path());
}

#[test]
fn rejects_record_after_terminal() {
    let dir = TempDir::new().unwrap();
    let epoch = init(dir.path());
    let id = Uuid::now_v7();
    write_records(
        dir.path(),
        &[
            created(epoch),
            accepted(id),
            started_rec(id),
            finished_rec(id),
            started_rec(id),
        ],
    );
    assert_corrupt(dir.path());
}

#[test]
fn rejects_duplicate_journal_created() {
    let dir = TempDir::new().unwrap();
    let epoch = init(dir.path());
    write_records(dir.path(), &[created(epoch), created(epoch)]);
    assert_corrupt(dir.path());
}

#[test]
fn rejects_unknown_record_kind() {
    let dir = TempDir::new().unwrap();
    let epoch = init(dir.path());
    let created_frame = super::record::frame(&created(epoch)).unwrap();
    let payload = br#"{"kind":"teleport","format":1}"#;
    let mut bytes = created_frame;
    bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&sha2::Sha256::digest(payload));
    bytes.extend_from_slice(payload);
    std::fs::write(dir.path().join(JOURNAL_FILE), bytes).unwrap();
    super::commit::write(
        dir.path(),
        journal_len(dir.path()),
        DurableFaults::default(),
    )
    .unwrap();
    assert_corrupt(dir.path());
}

// ---- Pruning and mutation evidence -----------------------------------------

#[test]
fn recent_mutating_acceptances_survive_compaction() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let id = Uuid::now_v7();
    let journal = open(dir.path());
    journal
        .accept(&request(id, "container.restart", true), T0, BOOT, MONO)
        .unwrap();
    assert!(!journal.prune(0, MONO, BOOT).unwrap(), "nothing removable");
    assert!(matches!(
        state_of(&journal, id),
        ActionState::OutcomeUnknown { .. }
    ));
}

#[test]
fn global_limits_survive_compaction() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let journal = Journal::open_unlocked(
        dir.path(),
        BOOT,
        MONO,
        MutationLimits {
            per_hour: 2,
            per_capability: BTreeMap::new(),
        },
    )
    .unwrap();
    finished_execution(&journal, Uuid::now_v7());
    finished_execution(&journal, Uuid::now_v7());
    assert!(
        !journal.prune(0, MONO, BOOT).unwrap(),
        "active evidence stays"
    );
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
fn per_capability_limits_survive_compaction() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let mut limits = limits();
    limits
        .per_capability
        .insert("container.restart".to_owned(), 1);
    let journal = Journal::open_unlocked(dir.path(), BOOT, MONO, limits).unwrap();
    finished_execution(&journal, Uuid::now_v7());
    assert!(!journal.prune(0, MONO, BOOT).unwrap());
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
fn old_acceptance_can_be_removed() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let id = Uuid::now_v7();
    let journal = open(dir.path());
    finished_execution(&journal, id);
    assert!(
        journal
            .prune(0, MONO + super::limiter::WINDOW_NS + 1, BOOT)
            .unwrap()
    );
    assert!(matches!(
        state_of(&journal, id),
        ActionState::OutcomeUnknown {
            reason
        } if reason.known() == Some(&OutcomeUnknownReason::JournalLost)
    ));
}

#[test]
fn noop_prune_does_not_set_marker() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let journal = open(dir.path());
    assert!(!journal.prune(0, MONO, BOOT).unwrap());
    let meta = super::Metadata::load(dir.path()).unwrap().unwrap();
    assert!(!meta.pruned);
}

#[test]
fn failed_directory_sync_never_causes_not_found() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let journal = open(dir.path());
    finished_execution(&journal, Uuid::now_v7());
    journal.inject_rewrite_dir_sync_failure();
    assert!(
        !journal
            .prune(0, MONO + super::limiter::WINDOW_NS + 1, BOOT)
            .unwrap()
    );
    assert!(!matches!(
        state_of(&journal, Uuid::now_v7()),
        ActionState::NotFound
    ));
}

#[test]
fn second_execution_of_a_finished_id_is_a_duplicate() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let journal = open(dir.path());
    let id = Uuid::now_v7();
    finished_execution(&journal, id);
    assert!(matches!(
        journal
            .accept(&request(id, "container.restart", true), T0, BOOT, MONO)
            .unwrap(),
        AcceptOutcome::Duplicate(_)
    ));
}

// ---- Cross-process ownership -----------------------------------------------

#[cfg(target_os = "linux")]
async fn wait_for_pid_async(path: &Path) -> u32 {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        // Existence is not enough: the shell creates the file before writing to it.
        if let Ok(text) = std::fs::read_to_string(path)
            && let Ok(pid) = text.trim().parse::<u32>()
        {
            return pid;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the child process did not report its pid"
        );
        tokio::task::yield_now().await;
    }
}

#[cfg(target_os = "linux")]
async fn wait_for_file_async(path: &Path) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !path.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "the child process did not signal"
        );
        tokio::task::yield_now().await;
    }
}

/// The child half of the cross-process ownership test; a no-op when not spawned by it.
#[cfg(target_os = "linux")]
#[test]
fn journal_lock_child() {
    let Ok(dir) = std::env::var("DOMOVOI_JOURNAL_CHILD_DIR") else {
        return;
    };
    let dir = Path::new(&dir);
    let journal = Journal::open(dir, BOOT, MONO, limits()).expect("the child owns the journal");
    std::fs::write(dir.join("hold"), b"held").unwrap();
    let release = dir.join("release");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !release.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "the parent did not release the child"
        );
        std::thread::yield_now();
    }
    drop(journal);
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn another_process_holding_the_journal_blocks_startup_and_local_commands() {
    use std::sync::Arc;

    use domovoi_core::ports::{ProcessExecutor as _, ProcessSpec};

    let dir = TempDir::new().unwrap();
    init(dir.path());
    let executor =
        domovoi_runner::SandboxExecutor::new(Arc::new(domovoi_testkit::ListRedactor::new(Vec::<
            String,
        >::new(
        ))));
    let spec = ProcessSpec {
        program: std::env::current_exe().unwrap().display().to_string(),
        args: vec![
            "--exact".to_owned(),
            "journal::tests::journal_lock_child".to_owned(),
            "--nocapture".to_owned(),
        ],
        env: BTreeMap::from([(
            "DOMOVOI_JOURNAL_CHILD_DIR".to_owned(),
            dir.path().display().to_string(),
        )]),
        cwd: Some(dir.path().display().to_string()),
        stdin: None,
        timeout: SignedDuration::from_secs(60),
        max_output_bytes: 4096,
        run_as: None,
    };
    let child = tokio::spawn(async move { executor.run(spec).await });
    wait_for_file_async(&dir.path().join("hold")).await;

    assert!(matches!(
        Journal::open(dir.path(), BOOT, MONO, limits()),
        Err(JournalError::Locked(_))
    ));
    assert!(matches!(
        Journal::init(dir.path(), T0, BOOT, MONO),
        Err(JournalError::Locked(_))
    ));
    assert!(matches!(
        Journal::reset(dir.path(), T0, BOOT, MONO),
        Err(JournalError::Locked(_))
    ));

    std::fs::write(dir.path().join("release"), b"go").unwrap();
    let output = child.await.expect("the child task joins");
    assert!(
        output.as_ref().is_ok(),
        "the child exits cleanly: {output:?}"
    );
    assert!(Journal::open(dir.path(), BOOT, MONO, limits()).is_ok());
}

// ---- Orphan recovery (Linux process groups) --------------------------------

#[cfg(target_os = "linux")]
struct RecordingClock {
    now: Timestamp,
    slept: std::sync::Mutex<Vec<Timestamp>>,
}

#[cfg(target_os = "linux")]
#[async_trait::async_trait]
impl domovoi_core::ports::Clock for RecordingClock {
    fn now(&self) -> Timestamp {
        self.now
    }

    async fn sleep_until(&self, at: Timestamp) {
        self.slept.lock().unwrap().push(at);
    }
}

#[cfg(target_os = "linux")]
fn records_for(dir: &Path, id: Uuid) -> Vec<Record> {
    let epoch = super::Metadata::load(dir).unwrap().unwrap().epoch;
    let bytes = std::fs::read(dir.join(JOURNAL_FILE)).unwrap();
    super::record::parse(&bytes, epoch)
        .unwrap()
        .records
        .into_iter()
        .filter(|record| record.execution_id() == Some(id))
        .collect()
}

#[cfg(target_os = "linux")]
fn start_orphan(journal: &Journal, id: Uuid, pid: u32, proc_start_time: u64) {
    journal
        .accept(&request(id, "container.restart", true), T0, BOOT, MONO)
        .unwrap();
    journal.started(id, pid, proc_start_time).unwrap();
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn orphan_process_group_is_terminated_after_the_grace_interval() {
    use std::os::unix::fs::PermissionsExt as _;
    use std::sync::Arc;

    use domovoi_core::ports::{ProcessExecutor as _, ProcessSpec};

    let dir = TempDir::new().unwrap();
    init(dir.path());
    let script = dir.path().join("orphan.sh");
    std::fs::write(&script, "#!/bin/sh\necho $$ > pid\nsleep 300\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let executor =
        domovoi_runner::SandboxExecutor::new(Arc::new(domovoi_testkit::ListRedactor::new(Vec::<
            String,
        >::new(
        ))));
    let spec = ProcessSpec {
        program: script.display().to_string(),
        args: Vec::new(),
        env: BTreeMap::new(),
        cwd: Some(dir.path().display().to_string()),
        stdin: None,
        timeout: SignedDuration::from_secs(60),
        max_output_bytes: 4096,
        run_as: None,
    };
    let running = tokio::spawn(async move { executor.run(spec).await });
    let pid = wait_for_pid_async(&dir.path().join("pid")).await;
    let start_time = super::recovery::process_stat(pid)
        .expect("the child is running")
        .start_time;
    let id = Uuid::now_v7();
    {
        let journal = open(dir.path());
        start_orphan(&journal, id, pid, start_time);
    }
    // A fresh open is a restart: nothing is running in this instance.
    let journal = open(dir.path());

    let clock = RecordingClock {
        now: T0,
        slept: std::sync::Mutex::new(Vec::new()),
    };
    journal.recover_orphans(&clock).await;

    let output = running.await.expect("the executor task joins");
    assert!(
        output.is_err() || output.as_ref().is_ok_and(|out| out.signal.is_some()),
        "the child was signalled: {output:?}"
    );
    let slept = clock.slept.lock().unwrap().clone();
    assert_eq!(
        slept.first(),
        Some(&(T0 + SignedDuration::from_secs(2))),
        "the grace interval is two seconds before the first post-grace probe"
    );
    assert_eq!(
        records_for(dir.path(), id)
            .into_iter()
            .filter(|record| matches!(record, Record::Abandoned { .. }))
            .count(),
        1,
        "the recovery is recorded durably"
    );
    assert!(matches!(
        state_of(&journal, id),
        ActionState::OutcomeUnknown {
            reason
        } if reason.known() == Some(&OutcomeUnknownReason::AgentRestarted)
    ));
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn pid_reuse_never_signals_an_unrelated_process() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let journal = open(dir.path());
    let id = Uuid::now_v7();
    // Our own PID exists but its recorded start time is wrong, so it is not the original.
    start_orphan(&journal, id, std::process::id(), 0);

    let clock = RecordingClock {
        now: T0,
        slept: std::sync::Mutex::new(Vec::new()),
    };
    journal.recover_orphans(&clock).await;

    assert!(
        clock.slept.lock().unwrap().is_empty(),
        "nothing was signalled"
    );
    assert!(
        records_for(dir.path(), id)
            .iter()
            .all(|record| !matches!(record, Record::Abandoned { .. })),
        "no abandoned record for a reused PID"
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn missing_process_produces_outcome_unknown_without_a_record() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let id = Uuid::now_v7();
    // No process can own this PID.
    let pid = u32::MAX;
    {
        let journal = open(dir.path());
        start_orphan(&journal, id, pid, 1);
    }
    let journal = open(dir.path());

    let clock = RecordingClock {
        now: T0,
        slept: std::sync::Mutex::new(Vec::new()),
    };
    journal.recover_orphans(&clock).await;

    assert!(clock.slept.lock().unwrap().is_empty());
    assert!(matches!(
        state_of(&journal, id),
        ActionState::OutcomeUnknown {
            reason
        } if reason.known() == Some(&OutcomeUnknownReason::AgentRestarted)
    ));
}

#[test]
fn more_than_ten_thousand_active_records_are_retained() {
    let dir = TempDir::new().unwrap();
    let epoch = init(dir.path());
    let first = Uuid::now_v7();
    let mut records = vec![created(epoch), accepted(first)];
    for _ in 1..10_001 {
        records.push(accepted(Uuid::now_v7()));
    }
    write_records(dir.path(), &records);
    let journal = open(dir.path());
    assert!(
        !journal.prune(0, MONO, BOOT).unwrap(),
        "active limiter evidence cannot be removed to satisfy a size policy"
    );
    assert!(matches!(
        state_of(&journal, first),
        ActionState::OutcomeUnknown {
            reason
        } if reason.known() == Some(&OutcomeUnknownReason::NoStartRecord)
    ));
}

#[test]
fn pruning_cannot_race_action_status_queries() {
    use std::sync::Arc;

    let dir = TempDir::new().unwrap();
    init(dir.path());
    let journal = Arc::new(open(dir.path()));
    finished_execution(&journal, Uuid::now_v7());
    let reader = {
        let journal = journal.clone();
        std::thread::spawn(move || {
            for _ in 0..200 {
                let _ = journal.action_status(&query(journal.epoch(), Uuid::now_v7()));
            }
        })
    };
    let pruner = {
        let journal = journal.clone();
        std::thread::spawn(move || {
            for _ in 0..50 {
                let _ = journal.prune(0, MONO + super::limiter::WINDOW_NS + 1, BOOT);
            }
        })
    };
    reader.join().expect("the reader finishes");
    pruner.join().expect("the pruner finishes");
}

#[test]
fn metadata_epoch_mismatch_fences_the_journal() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let mut meta = super::Metadata::load(dir.path()).unwrap().unwrap();
    let new_epoch = Uuid::now_v7();
    meta.epoch = new_epoch;
    meta.write(dir.path(), DurableFaults::default()).unwrap();
    let journal = open(dir.path());
    assert_eq!(journal.epoch(), new_epoch, "the visible epoch is preserved");
    assert!(
        journal.corruption().is_some(),
        "the journal no longer matches the metadata epoch"
    );
    assert!(matches!(
        state_of(&journal, Uuid::now_v7()),
        ActionState::OutcomeUnknown {
            reason
        } if reason.known() == Some(&OutcomeUnknownReason::JournalLost)
    ));
}

#[test]
fn fenced_lifecycle_operations_are_refused() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let journal = open(dir.path());
    let id = Uuid::now_v7();
    journal
        .accept(&request(id, "container.restart", true), T0, BOOT, MONO)
        .unwrap();
    journal.fence("injected permanent corruption");
    assert!(matches!(
        journal.accept(
            &request(Uuid::now_v7(), "container.restart", true),
            T0,
            BOOT,
            MONO
        ),
        Ok(AcceptOutcome::Refused(_))
    ));
    assert!(journal.started(id, 1, 1).is_err());
    assert!(journal.finished(id, result()).is_err());
    assert!(journal.prune(0, MONO, BOOT).is_err());
    assert!(matches!(
        state_of(&journal, Uuid::now_v7()),
        ActionState::OutcomeUnknown { .. }
    ));
}

#[test]
fn append_reconciliation_never_clears_a_permanent_fence() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let journal = open(dir.path());
    journal.fence("injected permanent corruption");
    journal.inject_append_fault(AppendFault::Uncertain);
    assert!(matches!(
        journal.accept(
            &request(Uuid::now_v7(), "container.restart", true),
            T0,
            BOOT,
            MONO
        ),
        Ok(AcceptOutcome::Refused(_))
    ));
    assert!(journal.corruption().is_some());
}

fn archive_names(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| {
            name.starts_with("action-journal.")
                && name != "action-journal.commit"
                && !name.ends_with(".bin")
                && !name.ends_with(".json")
                && !name.ends_with(".lock")
                && !name.ends_with(".reset")
        })
        .collect()
}

#[test]
fn reset_refuses_when_both_files_are_absent() {
    let dir = TempDir::new().unwrap();
    assert!(Journal::reset_unlocked(dir.path(), T0, BOOT, MONO).is_err());
    assert!(!dir.path().join(JOURNAL_FILE).exists());
    assert!(!dir.path().join(super::META_FILE).exists());
}

#[test]
fn reset_archives_a_missing_metadata_pair() {
    let dir = TempDir::new().unwrap();
    let old = init(dir.path());
    std::fs::remove_file(dir.path().join(super::META_FILE)).unwrap();
    let new = Journal::reset_unlocked(dir.path(), T0, BOOT, MONO).unwrap();
    assert_ne!(new, old);
    assert!(
        archive_names(dir.path())
            .iter()
            .any(|name| name.starts_with(JOURNAL_FILE)),
        "the journal was archived"
    );
}

#[test]
fn reset_archives_corrupt_metadata() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    std::fs::write(dir.path().join(super::META_FILE), b"not json").unwrap();
    let new = Journal::reset_unlocked(dir.path(), T0, BOOT, MONO).unwrap();
    assert!(!new.is_nil());
    assert!(
        archive_names(dir.path())
            .iter()
            .any(|name| name.starts_with(super::META_FILE)),
        "the corrupt metadata was archived"
    );
    assert!(Journal::open_unlocked(dir.path(), BOOT, MONO, limits()).is_ok());
}

#[test]
fn reset_never_overwrites_an_existing_archive() {
    let dir = TempDir::new().unwrap();
    let old = init(dir.path());
    let existing = dir.path().join(format!("{JOURNAL_FILE}.{old}"));
    std::fs::write(&existing, b"earlier archive").unwrap();
    Journal::reset_unlocked(dir.path(), T0, BOOT, MONO).unwrap();
    assert_eq!(std::fs::read(&existing).unwrap(), b"earlier archive");
    let archives: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(&format!("{JOURNAL_FILE}."))
        })
        .collect();
    assert_eq!(
        archives.len(),
        2,
        "the old archive and a new one both exist"
    );
}

#[test]
fn repeated_resets_keep_unique_archives() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let first = Journal::reset_unlocked(dir.path(), T0, BOOT, MONO).unwrap();
    let second = Journal::reset_unlocked(dir.path(), T0, BOOT, MONO).unwrap();
    assert_ne!(first, second);
    let archives: Vec<String> = archive_names(dir.path())
        .into_iter()
        .filter(|name| name.starts_with(JOURNAL_FILE))
        .collect();
    assert_eq!(archives.len(), 2, "each reset preserved its own pair");
    assert!(dir.path().join(format!("{JOURNAL_FILE}.{first}")).exists());
}

#[test]
fn an_interrupted_reset_can_be_resumed() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let epoch = Uuid::now_v7();
    std::fs::write(
        dir.path().join(super::store::RESET_INTENT_FILE),
        serde_json::to_vec(&serde_json::json!({
            "stage": "archiving",
            "generation": "g",
            "sources": [JOURNAL_FILE, super::META_FILE, super::commit::COMMIT_FILE],
            "epoch": epoch.to_string(),
        }))
        .unwrap(),
    )
    .unwrap();
    let resumed = Journal::reset_unlocked(dir.path(), T0, BOOT, MONO).unwrap();
    assert_eq!(resumed, epoch, "the epoch chosen before archival is reused");
    assert!(!dir.path().join(super::store::RESET_INTENT_FILE).exists());
    assert!(dir.path().join(format!("{JOURNAL_FILE}.g")).exists());
    assert!(dir.path().join(format!("{}.g", super::META_FILE)).exists());
    assert!(Journal::open_unlocked(dir.path(), BOOT, MONO, limits()).is_ok());
}

#[test]
fn open_refuses_an_interrupted_reset() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    std::fs::write(
        dir.path().join(super::store::RESET_INTENT_FILE),
        b"interrupted",
    )
    .unwrap();
    let err = Journal::open_unlocked(dir.path(), BOOT, MONO, limits()).unwrap_err();
    assert!(err.to_string().contains("interrupted"), "{err}");
}

#[test]
fn nil_persisted_metadata_epoch_is_rejected() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let mut meta = super::Metadata::load(dir.path()).unwrap().unwrap();
    meta.epoch = Uuid::nil();
    meta.write(dir.path(), DurableFaults::default()).unwrap();
    assert!(super::Metadata::load(dir.path()).is_err());
}

#[test]
fn nil_journal_created_epoch_is_rejected() {
    let frame = super::record::frame(&created(Uuid::nil())).unwrap();
    assert!(super::record::parse(&frame, Uuid::nil()).is_err());
}

#[test]
fn a_poisoned_state_mutex_cannot_hide_the_epoch() {
    let dir = TempDir::new().unwrap();
    let epoch = init(dir.path());
    let journal = open(dir.path());
    journal.poison_state_for_test();
    assert_eq!(journal.epoch(), epoch);
    assert!(
        journal
            .action_status(&query(epoch, Uuid::now_v7()))
            .is_err()
    );
}

#[test]
fn unconfirmed_finished_frame_is_not_adopted_after_restart() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let id = Uuid::now_v7();
    let committed;
    {
        let journal = open(dir.path());
        journal
            .accept(&request(id, "container.restart", true), T0, BOOT, MONO)
            .unwrap();
        journal.started(id, 1, 1).unwrap();
        committed = committed_len(dir.path());
        journal.inject_append_fault(AppendFault::RecoverySyncTruncateFail);
        assert!(journal.finished(id, result()).is_err());
        assert!(!matches!(
            state_of(&journal, id),
            ActionState::Finished { .. }
        ));
    }
    // The durable boundary still names the started record; the finished frame lies beyond it
    // and the process's fence disappeared with the process.
    assert_eq!(committed_len(dir.path()), committed);
    assert!(journal_len(dir.path()) > committed);
    let reopened = open(dir.path());
    assert!(!matches!(
        state_of(&reopened, id),
        ActionState::Finished { .. }
    ));
    assert_eq!(
        journal_len(dir.path()),
        committed,
        "startup truncated everything past the committed boundary"
    );
}

#[test]
fn unconfirmed_acceptance_does_not_spawn_after_restart() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let id = Uuid::now_v7();
    let committed = committed_len(dir.path());
    {
        let journal = open(dir.path());
        journal.inject_append_fault(AppendFault::RecoverySyncTruncateFail);
        assert!(
            journal
                .accept(&request(id, "container.restart", true), T0, BOOT, MONO)
                .is_err()
        );
    }
    assert_eq!(committed_len(dir.path()), committed);
    let reopened = open(dir.path());
    assert_eq!(
        state_of(&reopened, id),
        ActionState::NotFound,
        "an uncommitted acceptance is never adopted"
    );
}

#[test]
fn indeterminate_boot_write_is_confirmed_by_recovery_sync() {
    let dir = TempDir::new().unwrap();
    let epoch = init(dir.path());
    let journal = Journal::open_unlocked_with_boot_faults(
        dir.path(),
        BOOT_B,
        5 * MONO,
        limits(),
        DurableFaults {
            dir_sync: true,
            ..DurableFaults::default()
        },
    )
    .unwrap();
    assert_eq!(journal.epoch(), epoch);
    assert!(journal.corruption().is_none());
    let meta = super::Metadata::load(dir.path()).unwrap().unwrap();
    assert_eq!(meta.boot.boot_id, BOOT_B);
    assert_eq!(meta.boot.first_load_mono_ns, 5 * MONO);
}

#[test]
fn unconfirmed_boot_write_fails_startup_without_clearing_the_anchor() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let result = Journal::open_unlocked_with_boot_faults(
        dir.path(),
        BOOT_B,
        5 * MONO,
        limits(),
        DurableFaults {
            rename: true,
            ..DurableFaults::default()
        },
    );
    assert!(result.is_err(), "an unconfirmed boot write is not adopted");
    let meta = super::Metadata::load(dir.path()).unwrap().unwrap();
    assert_eq!(meta.boot.boot_id, BOOT, "the old anchor is intact");
    // A later successful open commits the new boot state.
    let journal = Journal::open_unlocked(dir.path(), BOOT_B, 5 * MONO, limits()).unwrap();
    assert!(journal.corruption().is_none());
    let meta = super::Metadata::load(dir.path()).unwrap().unwrap();
    assert_eq!(meta.boot.boot_id, BOOT_B);
}

#[test]
fn reset_after_a_created_epoch_only_finishes_the_intent() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let epoch = Journal::reset_unlocked(dir.path(), T0, BOOT, MONO).unwrap();
    let archives_before = archive_names(dir.path()).len();
    std::fs::write(
        dir.path().join(super::store::RESET_INTENT_FILE),
        serde_json::to_vec(&serde_json::json!({
            "stage": "created",
            "generation": "g",
            "epoch": epoch.to_string(),
        }))
        .unwrap(),
    )
    .unwrap();
    let again = Journal::reset_unlocked(dir.path(), T0, BOOT, MONO).unwrap();
    assert_eq!(again, epoch, "the created epoch is not rotated");
    assert_eq!(archive_names(dir.path()).len(), archives_before);
    assert!(!dir.path().join(super::store::RESET_INTENT_FILE).exists());
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn leader_exits_before_recovery_but_a_descendant_is_still_terminated() {
    use std::os::unix::fs::PermissionsExt as _;
    use std::sync::Arc;

    use domovoi_core::ports::{ProcessExecutor as _, ProcessSpec};

    let dir = TempDir::new().unwrap();
    init(dir.path());
    let script = dir.path().join("leader-exits.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\n(trap '' TERM; while :; do sleep 1; done) &\necho $! > child\necho $$ > pid\nwhile [ ! -e go ]; do sleep 0.05; done\nexit 0\n",
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let executor =
        domovoi_runner::SandboxExecutor::new(Arc::new(domovoi_testkit::ListRedactor::new(Vec::<
            String,
        >::new(
        ))));
    let spec = ProcessSpec {
        program: script.display().to_string(),
        args: Vec::new(),
        env: BTreeMap::new(),
        cwd: Some(dir.path().display().to_string()),
        stdin: None,
        timeout: SignedDuration::from_secs(60),
        max_output_bytes: 4096,
        run_as: None,
    };
    let running = tokio::spawn(async move { executor.run(spec).await });
    let pid = wait_for_pid_async(&dir.path().join("pid")).await;
    let _child = wait_for_pid_async(&dir.path().join("child")).await;
    let stat = super::recovery::process_stat(pid).expect("the leader is running");
    let id = Uuid::now_v7();
    {
        let journal = open(dir.path());
        start_orphan(&journal, id, pid, stat.start_time);
    }
    std::fs::write(dir.path().join("go"), b"go").unwrap();
    let _ = running.await.expect("the executor task joins");

    let journal = open(dir.path());
    let clock = RecordingClock {
        now: T0,
        slept: std::sync::Mutex::new(Vec::new()),
    };
    journal.recover_orphans(&clock).await;

    assert!(
        matches!(
            super::recovery::group_state(pid, stat.session),
            super::recovery::GroupState::Empty
        ),
        "the surviving descendant's group is gone"
    );
    assert!(matches!(
        state_of(&journal, id),
        ActionState::OutcomeUnknown {
            reason
        } if reason.known() == Some(&OutcomeUnknownReason::AgentRestarted)
    ));
}

// ---- Commit boundary -------------------------------------------------------

#[test]
fn append_open_failure_leaves_the_boundary_and_records_unchanged() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let journal = open(dir.path());
    let committed = committed_len(dir.path());
    let id = Uuid::now_v7();
    journal.inject_append_fault(AppendFault::Open);
    assert!(
        journal
            .accept(&request(id, "container.restart", true), T0, BOOT, MONO)
            .is_err()
    );
    assert_eq!(committed_len(dir.path()), committed);
    assert_eq!(journal_len(dir.path()), committed);
    assert!(journal.corruption().is_none());
    assert_eq!(state_of(&journal, id), ActionState::NotFound);
}

#[test]
fn a_missing_commit_boundary_fails_startup_closed() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    std::fs::remove_file(dir.path().join(super::commit::COMMIT_FILE)).unwrap();
    assert!(
        Journal::open_unlocked(dir.path(), BOOT, MONO, limits()).is_err(),
        "a journal without a boundary has no acknowledged bytes"
    );
}

#[test]
fn a_complete_frame_beyond_the_boundary_is_truncated_at_startup() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let id = Uuid::now_v7();
    let committed;
    {
        let journal = open(dir.path());
        journal
            .accept(&request(id, "container.restart", true), T0, BOOT, MONO)
            .unwrap();
        journal.started(id, 1, 1).unwrap();
        committed = committed_len(dir.path());
    }
    // Simulate a crash after the frame was written and synced but before the boundary was
    // replaced: the bytes are complete and checksum-valid, yet nobody acknowledged them.
    let path = dir.path().join(JOURNAL_FILE);
    let mut bytes = std::fs::read(&path).unwrap();
    bytes.extend_from_slice(&super::record::frame(&finished_rec(id)).unwrap());
    std::fs::write(&path, bytes).unwrap();
    let reopened = open(dir.path());
    assert_eq!(committed_len(dir.path()), committed);
    assert_eq!(journal_len(dir.path()), committed);
    assert!(!matches!(
        state_of(&reopened, id),
        ActionState::Finished { .. }
    ));
}

#[test]
fn a_boundary_write_failure_never_acknowledges_the_append() {
    for (stage, faults) in [
        (
            "temporary-file sync",
            DurableFaults {
                temp_sync: true,
                ..DurableFaults::default()
            },
        ),
        (
            "rename",
            DurableFaults {
                rename: true,
                ..DurableFaults::default()
            },
        ),
    ] {
        let dir = TempDir::new().unwrap();
        init(dir.path());
        let journal = open(dir.path());
        let accepted = Uuid::now_v7();
        let committed = committed_len(dir.path());
        journal.inject_commit_faults(faults);
        assert!(
            journal
                .accept(
                    &request(accepted, "container.restart", true),
                    T0,
                    BOOT,
                    MONO
                )
                .is_err(),
            "{stage}: the append is not acknowledged"
        );
        assert_eq!(committed_len(dir.path()), committed, "{stage}");
        assert_eq!(journal_len(dir.path()), committed, "{stage}");
        assert!(journal.corruption().is_none(), "{stage}");
        let reopened = open(dir.path());
        assert_eq!(
            state_of(&reopened, accepted),
            ActionState::NotFound,
            "{stage}"
        );
    }
}

#[test]
fn an_unconfirmed_boundary_is_confirmed_by_a_recovery_sync() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let journal = open(dir.path());
    let id = Uuid::now_v7();
    journal.inject_commit_faults(DurableFaults {
        dir_sync: true,
        ..DurableFaults::default()
    });
    assert_eq!(
        journal
            .accept(&request(id, "container.restart", true), T0, BOOT, MONO)
            .unwrap(),
        AcceptOutcome::Accepted,
        "the recovery sync proved the boundary durable"
    );
    assert!(journal.corruption().is_none());
    let reopened = open(dir.path());
    assert!(matches!(
        state_of(&reopened, id),
        ActionState::OutcomeUnknown { .. }
    ));
}

#[test]
fn an_unconfirmed_boundary_without_confirmation_is_rolled_back() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let id = Uuid::now_v7();
    let committed = committed_len(dir.path());
    {
        let journal = open(dir.path());
        journal.inject_commit_faults(DurableFaults {
            dir_sync: true,
            ..DurableFaults::default()
        });
        journal.inject_commit_confirm_failure();
        assert!(
            journal
                .accept(&request(id, "container.restart", true), T0, BOOT, MONO)
                .is_err()
        );
    }
    assert_eq!(committed_len(dir.path()), committed);
    let reopened = open(dir.path());
    assert_eq!(state_of(&reopened, id), ActionState::NotFound);
}

#[test]
fn failed_boundary_and_failed_truncation_never_adopt_after_restart() {
    // The exact dangerous case: a complete synced frame, the boundary replacement fails,
    // and the repair truncation fails too. The process then dies.
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let kept = Uuid::now_v7();
    let lost = Uuid::now_v7();
    let committed;
    {
        let journal = open(dir.path());
        journal
            .accept(&request(kept, "container.restart", true), T0, BOOT, MONO)
            .unwrap();
        committed = committed_len(dir.path());
        journal.inject_append_fault(AppendFault::Uncertain);
        journal.inject_commit_faults(DurableFaults {
            rename: true,
            ..DurableFaults::default()
        });
        journal.inject_truncate_failure();
        assert!(
            journal
                .accept(&request(lost, "container.restart", true), T0, BOOT, MONO)
                .is_err()
        );
        assert!(journal.corruption().is_some(), "the process is fenced");
    }
    // The frame is still physically present and parses on its own, but no boundary names it.
    assert_eq!(committed_len(dir.path()), committed);
    assert!(journal_len(dir.path()) > committed);
    let reopened = open(dir.path());
    assert!(
        matches!(
            state_of(&reopened, kept),
            ActionState::OutcomeUnknown { reason }
                if reason.known() == Some(&OutcomeUnknownReason::NoStartRecord)
        ),
        "the previously committed record survives"
    );
    assert_eq!(
        state_of(&reopened, lost),
        ActionState::NotFound,
        "the unconfirmed frame never becomes authoritative"
    );
    assert_eq!(
        journal_len(dir.path()),
        committed,
        "startup truncated the unconfirmed frame"
    );
}

// ---- Shared metadata reconciliation ---------------------------------------

#[test]
fn reconciliation_accepts_only_the_exact_intended_metadata() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let meta = super::Metadata::load(dir.path()).unwrap().unwrap();
    let reconciled = super::store::reconcile_metadata(dir.path(), &meta).unwrap();
    assert_eq!(reconciled.meta, meta);
    assert!(reconciled.fence.is_none());

    // Same epoch, different sticky state: this is not the intended object.
    let mut wrong = meta.clone();
    wrong.pruned = true;
    let err = super::store::reconcile_metadata(dir.path(), &wrong).unwrap_err();
    assert!(err.to_string().contains("not the intended value"), "{err}");
}

#[test]
fn reconciliation_rejects_corrupt_metadata() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let meta = super::Metadata::load(dir.path()).unwrap().unwrap();
    std::fs::write(dir.path().join(super::META_FILE), b"{ not json").unwrap();
    assert!(super::store::reconcile_metadata(dir.path(), &meta).is_err());
}

#[test]
fn reconciliation_fences_an_inconsistent_journal() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let meta = super::Metadata::load(dir.path()).unwrap().unwrap();
    std::fs::remove_file(dir.path().join(JOURNAL_FILE)).unwrap();
    let reconciled = super::store::reconcile_metadata(dir.path(), &meta).unwrap();
    assert!(
        reconciled.fence.is_some(),
        "valid metadata with a missing journal is fenced, not adopted as healthy"
    );
}

#[test]
fn metadata_write_failures_before_replacement_leave_the_old_value() {
    for (stage, faults) in [
        (
            "temporary-file sync",
            DurableFaults {
                temp_sync: true,
                ..DurableFaults::default()
            },
        ),
        (
            "rename",
            DurableFaults {
                rename: true,
                ..DurableFaults::default()
            },
        ),
    ] {
        let dir = TempDir::new().unwrap();
        init(dir.path());
        let id = Uuid::now_v7();
        let journal = open(dir.path());
        finished_execution(&journal, id);
        journal.inject_metadata_faults(faults);
        assert!(
            journal
                .prune(0, MONO + super::limiter::WINDOW_NS + 1, BOOT)
                .is_err(),
            "{stage}"
        );
        assert!(
            journal.corruption().is_none(),
            "{stage}: nothing was renamed"
        );
        let meta = super::Metadata::load(dir.path()).unwrap().unwrap();
        assert!(!meta.pruned, "{stage}");
        assert!(matches!(
            state_of(&journal, id),
            ActionState::Finished { .. }
        ));
    }
}

#[test]
fn an_indeterminate_metadata_write_adopts_the_confirmed_intended_value() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let journal = open(dir.path());
    finished_execution(&journal, Uuid::now_v7());
    journal.inject_metadata_faults(DurableFaults {
        dir_sync: true,
        ..DurableFaults::default()
    });
    assert!(
        !journal
            .prune(0, MONO + super::limiter::WINDOW_NS + 1, BOOT)
            .unwrap(),
        "the rewrite did not report removed records"
    );
    assert!(
        journal.corruption().is_none(),
        "the recovery sync confirmed the intended metadata"
    );
    let meta = super::Metadata::load(dir.path()).unwrap().unwrap();
    assert!(
        meta.pruned,
        "the sticky marker is adopted only after confirmation"
    );
}

// ---- Reset archive ordering ------------------------------------------------

fn has_original(dir: &Path, name: &str, original: &[u8]) -> bool {
    if std::fs::read(dir.join(name)).ok().as_deref() == Some(original) {
        return true;
    }
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .any(|entry| {
            let candidate = entry.file_name().to_string_lossy().into_owned();
            candidate.starts_with(&format!("{name}."))
                && std::fs::read(entry.path()).ok().as_deref() == Some(original)
        })
}

#[test]
fn every_archive_failure_keeps_a_recoverable_copy_and_resumes() {
    let cases = [
        (
            "intent write",
            super::reset::ResetFaults {
                intent_write: true,
                ..super::reset::ResetFaults::default()
            },
        ),
        (
            "first link",
            super::reset::ResetFaults {
                link_first: true,
                ..super::reset::ResetFaults::default()
            },
        ),
        (
            "second link",
            super::reset::ResetFaults {
                link_second: true,
                ..super::reset::ResetFaults::default()
            },
        ),
        (
            "link verification",
            super::reset::ResetFaults {
                verify: true,
                ..super::reset::ResetFaults::default()
            },
        ),
        (
            "directory sync after links",
            super::reset::ResetFaults {
                link_dir_sync: true,
                ..super::reset::ResetFaults::default()
            },
        ),
        (
            "first source removal",
            super::reset::ResetFaults {
                remove_first: true,
                ..super::reset::ResetFaults::default()
            },
        ),
        (
            "second source removal",
            super::reset::ResetFaults {
                remove_second: true,
                ..super::reset::ResetFaults::default()
            },
        ),
        (
            "directory sync after removal",
            super::reset::ResetFaults {
                remove_dir_sync: true,
                ..super::reset::ResetFaults::default()
            },
        ),
        (
            "epoch creation",
            super::reset::ResetFaults {
                create_epoch: true,
                ..super::reset::ResetFaults::default()
            },
        ),
        (
            "epoch boundary",
            super::reset::ResetFaults {
                epoch_commit: DurableFaults {
                    rename: true,
                    ..DurableFaults::default()
                },
                ..super::reset::ResetFaults::default()
            },
        ),
        (
            "intent removal",
            super::reset::ResetFaults {
                intent_remove: true,
                ..super::reset::ResetFaults::default()
            },
        ),
        (
            "intent directory sync",
            super::reset::ResetFaults {
                intent_dir_sync: true,
                ..super::reset::ResetFaults::default()
            },
        ),
    ];
    for (stage, faults) in cases {
        let dir = TempDir::new().unwrap();
        init(dir.path());
        let journal = std::fs::read(dir.path().join(JOURNAL_FILE)).unwrap();
        let meta = std::fs::read(dir.path().join(super::META_FILE)).unwrap();
        let boundary = std::fs::read(dir.path().join(super::commit::COMMIT_FILE)).unwrap();
        assert!(
            Journal::reset_unlocked_with_faults(dir.path(), T0, BOOT, MONO, faults).is_err(),
            "{stage}: the reset must report failure"
        );
        assert!(has_original(dir.path(), JOURNAL_FILE, &journal), "{stage}");
        assert!(has_original(dir.path(), super::META_FILE, &meta), "{stage}");
        assert!(
            has_original(dir.path(), super::commit::COMMIT_FILE, &boundary),
            "{stage}"
        );
        // Retrying the interrupted reset finishes it without losing any copy.
        let epoch = Journal::reset_unlocked(dir.path(), T0, BOOT, MONO)
            .unwrap_or_else(|err| panic!("{stage}: retry failed: {err}"));
        assert!(!epoch.is_nil(), "{stage}");
        assert!(has_original(dir.path(), JOURNAL_FILE, &journal), "{stage}");
        assert!(has_original(dir.path(), super::META_FILE, &meta), "{stage}");
        assert!(
            has_original(dir.path(), super::commit::COMMIT_FILE, &boundary),
            "{stage}"
        );
        assert!(
            Journal::open_unlocked(dir.path(), BOOT, MONO, limits()).is_ok(),
            "{stage}: the reset journal opens"
        );
        assert!(
            !dir.path().join(super::store::RESET_INTENT_FILE).exists(),
            "{stage}"
        );
    }
}

#[test]
fn a_reset_resumes_after_links_are_durable_but_sources_remain() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let journal = std::fs::read(dir.path().join(JOURNAL_FILE)).unwrap();
    let meta = std::fs::read(dir.path().join(super::META_FILE)).unwrap();
    let epoch = Uuid::now_v7();
    // Simulate the crash window: links exist, sources are still present.
    for (name, bytes) in [
        (JOURNAL_FILE, &journal),
        (super::META_FILE, &meta),
        (
            super::commit::COMMIT_FILE,
            &std::fs::read(dir.path().join(super::commit::COMMIT_FILE)).unwrap(),
        ),
    ] {
        std::fs::hard_link(dir.path().join(name), dir.path().join(format!("{name}.g"))).unwrap();
        assert_eq!(
            std::fs::read(dir.path().join(format!("{name}.g"))).unwrap(),
            *bytes
        );
    }
    std::fs::write(
        dir.path().join(super::store::RESET_INTENT_FILE),
        serde_json::to_vec(&serde_json::json!({
            "stage": "archiving",
            "generation": "g",
            "sources": [JOURNAL_FILE, super::META_FILE, super::commit::COMMIT_FILE],
            "epoch": epoch.to_string(),
        }))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        Journal::reset_unlocked(dir.path(), T0, BOOT, MONO).unwrap(),
        epoch
    );
    assert!(has_original(dir.path(), JOURNAL_FILE, &journal));
    assert!(has_original(dir.path(), super::META_FILE, &meta));
    assert!(Journal::open_unlocked(dir.path(), BOOT, MONO, limits()).is_ok());
}

#[test]
fn a_reset_finishes_an_epoch_created_before_the_stage_write() {
    let dir = TempDir::new().unwrap();
    let epoch = Uuid::now_v7();
    super::store::create_epoch_with(dir.path(), epoch, T0, BOOT, MONO, DurableFaults::default())
        .unwrap();
    std::fs::write(
        dir.path().join(super::store::RESET_INTENT_FILE),
        serde_json::to_vec(&serde_json::json!({
            "stage": "archived",
            "generation": "g",
            "sources": [],
            "epoch": epoch.to_string(),
        }))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        Journal::reset_unlocked(dir.path(), T0, BOOT, MONO).unwrap(),
        epoch,
        "the already-created epoch is not rotated"
    );
    assert!(!dir.path().join(super::store::RESET_INTENT_FILE).exists());
}

#[test]
fn a_reset_refuses_a_journal_from_another_epoch() {
    let dir = TempDir::new().unwrap();
    let epoch = Uuid::now_v7();
    super::store::create_epoch_with(dir.path(), epoch, T0, BOOT, MONO, DurableFaults::default())
        .unwrap();
    std::fs::write(
        dir.path().join(super::store::RESET_INTENT_FILE),
        serde_json::to_vec(&serde_json::json!({
            "stage": "archived",
            "generation": "g",
            "sources": [],
            "epoch": Uuid::now_v7().to_string(),
        }))
        .unwrap(),
    )
    .unwrap();
    assert!(Journal::reset_unlocked(dir.path(), T0, BOOT, MONO).is_err());
    assert!(
        dir.path().join(super::store::RESET_INTENT_FILE).exists(),
        "the unresolved intent is kept"
    );
    let meta = super::Metadata::load(dir.path()).unwrap().unwrap();
    assert_eq!(meta.epoch, epoch, "the existing epoch is untouched");
}

#[test]
fn a_metadata_only_reset_archives_the_metadata_and_boundary() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let meta = std::fs::read(dir.path().join(super::META_FILE)).unwrap();
    std::fs::remove_file(dir.path().join(JOURNAL_FILE)).unwrap();
    let epoch = Journal::reset_unlocked(dir.path(), T0, BOOT, MONO).unwrap();
    assert!(!epoch.is_nil());
    assert!(has_original(dir.path(), super::META_FILE, &meta));
    assert!(Journal::open_unlocked(dir.path(), BOOT, MONO, limits()).is_ok());
}

// ---- Injected process inspection ------------------------------------------

struct SteppingClock {
    now: std::sync::Mutex<Timestamp>,
}

impl SteppingClock {
    fn new(now: Timestamp) -> Self {
        Self {
            now: std::sync::Mutex::new(now),
        }
    }
}

#[async_trait::async_trait]
impl domovoi_core::ports::Clock for SteppingClock {
    fn now(&self) -> Timestamp {
        *self.now.lock().unwrap()
    }

    async fn sleep_until(&self, at: Timestamp) {
        *self.now.lock().unwrap() = at;
    }
}

struct FakeInspector {
    procs: std::sync::Mutex<BTreeMap<u32, super::recovery::ProcessLookup>>,
    group: std::sync::Mutex<Option<bool>>,
    list_fails: bool,
    term: super::recovery::SignalOutcome,
    kill: super::recovery::SignalOutcome,
    deliver: bool,
    sent: std::sync::Mutex<Vec<(u32, super::recovery::Signal)>>,
}

impl FakeInspector {
    fn new() -> Self {
        Self {
            procs: std::sync::Mutex::new(BTreeMap::new()),
            group: std::sync::Mutex::new(Some(true)),
            list_fails: false,
            term: super::recovery::SignalOutcome::Sent,
            kill: super::recovery::SignalOutcome::Sent,
            deliver: true,
            sent: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn set(&self, pid: u32, lookup: super::recovery::ProcessLookup) {
        self.procs.lock().unwrap().insert(pid, lookup);
    }

    fn set_group(&self, exists: Option<bool>) {
        *self.group.lock().unwrap() = exists;
    }

    fn signals(&self) -> Vec<(u32, super::recovery::Signal)> {
        self.sent.lock().unwrap().clone()
    }

    fn live(start_time: u64, pgrp: u32, session: u32) -> super::recovery::ProcessLookup {
        super::recovery::ProcessLookup::Running(super::recovery::ProcessStat {
            state: 'S',
            start_time,
            pgrp,
            session,
        })
    }
}

impl super::recovery::ProcessInspector for FakeInspector {
    fn lookup(&self, pid: u32) -> super::recovery::ProcessLookup {
        self.procs
            .lock()
            .unwrap()
            .get(&pid)
            .copied()
            .unwrap_or(super::recovery::ProcessLookup::Gone)
    }

    fn list(&self) -> Result<Vec<u32>, super::recovery::InspectError> {
        if self.list_fails {
            return Err(super::recovery::InspectError::new(std::io::Error::other(
                "injected enumeration failure",
            )));
        }
        Ok(self.procs.lock().unwrap().keys().copied().collect())
    }

    fn group_exists(&self, _pgid: u32) -> Option<bool> {
        *self.group.lock().unwrap()
    }

    fn signal(&self, pgid: u32, signal: super::recovery::Signal) -> super::recovery::SignalOutcome {
        self.sent.lock().unwrap().push((pgid, signal));
        let outcome = match signal {
            super::recovery::Signal::Term => self.term,
            super::recovery::Signal::Kill => self.kill,
        };
        if outcome == super::recovery::SignalOutcome::Sent && self.deliver {
            let mut procs = self.procs.lock().unwrap();
            for lookup in procs.values_mut() {
                if matches!(
                    lookup,
                    super::recovery::ProcessLookup::Running(stat) if stat.pgrp == pgid
                ) {
                    *lookup = super::recovery::ProcessLookup::Gone;
                }
            }
            *self.group.lock().unwrap() = Some(false);
        }
        outcome
    }
}

fn started_at(id: Uuid, pid: u32, start: u64, pgid: u32, session: u32) -> Record {
    Record::Started {
        format: Record::FORMAT,
        execution_id: id,
        pid,
        proc_start_time: start,
        pgid,
        session,
    }
}

fn orphan_journal(dir: &Path, id: Uuid, pid: u32, start: u64) -> Journal {
    let epoch = init(dir);
    write_records(
        dir,
        &[
            created(epoch),
            accepted(id),
            started_at(id, pid, start, pid, 9),
        ],
    );
    open(dir)
}

fn abandoned_count(dir: &Path, id: Uuid) -> usize {
    let epoch = super::Metadata::load(dir).unwrap().unwrap().epoch;
    let bytes = match std::fs::read(dir.join(JOURNAL_FILE)) {
        Ok(bytes) => bytes,
        Err(_) => return 0,
    };
    super::record::parse(&bytes, epoch)
        .map(|parsed| {
            parsed
                .records
                .iter()
                .filter(|record| {
                    matches!(
                        record,
                        Record::Abandoned { execution_id, .. } if *execution_id == id
                    )
                })
                .count()
        })
        .unwrap_or(0)
}

#[tokio::test]
async fn an_unreadable_group_member_is_unknown_and_never_abandoned() {
    let dir = TempDir::new().unwrap();
    let id = Uuid::now_v7();
    let leader = 41_001;
    let member = 41_002;
    let journal = orphan_journal(dir.path(), id, leader, 7);
    let clock = SteppingClock::new(T0);
    let inspector = FakeInspector::new();
    inspector.set(member, super::recovery::ProcessLookup::Denied);
    journal.recover_orphans_with(&clock, &inspector).await;
    assert_eq!(
        abandoned_count(dir.path(), id),
        0,
        "an unreadable member means the group is not proven empty"
    );
    // A later complete scan proves the group gone.
    inspector.set(member, super::recovery::ProcessLookup::Gone);
    inspector.set_group(Some(false));
    journal.recover_orphans_with(&clock, &inspector).await;
    assert_eq!(
        abandoned_count(dir.path(), id),
        1,
        "abandonment follows a complete scan that proves emptiness"
    );
}

#[tokio::test]
async fn failed_and_malformed_process_reads_are_unknown() {
    for lookup in [
        super::recovery::ProcessLookup::Failed,
        super::recovery::ProcessLookup::Malformed,
    ] {
        let dir = TempDir::new().unwrap();
        let id = Uuid::now_v7();
        let journal = orphan_journal(dir.path(), id, 41_010, 7);
        let clock = SteppingClock::new(T0);
        let inspector = FakeInspector::new();
        inspector.set(41_011, lookup);
        journal.recover_orphans_with(&clock, &inspector).await;
        assert_eq!(abandoned_count(dir.path(), id), 0, "{lookup:?}");
    }
}

#[tokio::test]
async fn a_failed_process_enumeration_is_unknown() {
    let dir = TempDir::new().unwrap();
    let id = Uuid::now_v7();
    let journal = orphan_journal(dir.path(), id, 41_020, 7);
    let clock = SteppingClock::new(T0);
    let mut inspector = FakeInspector::new();
    inspector.list_fails = true;
    inspector.set_group(Some(false));
    journal.recover_orphans_with(&clock, &inspector).await;
    assert_eq!(abandoned_count(dir.path(), id), 0);
}

#[tokio::test]
async fn a_zombie_only_group_is_empty_and_abandoned() {
    let dir = TempDir::new().unwrap();
    let id = Uuid::now_v7();
    let journal = orphan_journal(dir.path(), id, 41_030, 7);
    let clock = SteppingClock::new(T0);
    let inspector = FakeInspector::new();
    inspector.set(
        41_031,
        super::recovery::ProcessLookup::Running(super::recovery::ProcessStat {
            state: 'Z',
            start_time: 9,
            pgrp: 41_030,
            session: 9,
        }),
    );
    journal.recover_orphans_with(&clock, &inspector).await;
    assert_eq!(
        abandoned_count(dir.path(), id),
        1,
        "a zombie cannot run and does not keep the group alive"
    );
}

#[tokio::test]
async fn a_live_group_is_terminated_then_abandoned() {
    let dir = TempDir::new().unwrap();
    let id = Uuid::now_v7();
    let pid = 41_040;
    let journal = orphan_journal(dir.path(), id, pid, 7);
    let clock = SteppingClock::new(T0);
    let inspector = FakeInspector::new();
    inspector.set(pid, FakeInspector::live(7, pid, 9));
    journal.recover_orphans_with(&clock, &inspector).await;
    assert_eq!(
        inspector.signals(),
        vec![(pid, super::recovery::Signal::Term)],
        "the live leader is terminated once"
    );
    assert_eq!(abandoned_count(dir.path(), id), 1);
}

#[tokio::test]
async fn a_failed_sigterm_never_abandons() {
    let dir = TempDir::new().unwrap();
    let id = Uuid::now_v7();
    let pid = 41_050;
    let journal = orphan_journal(dir.path(), id, pid, 7);
    let clock = SteppingClock::new(T0);
    let mut inspector = FakeInspector::new();
    inspector.set(pid, FakeInspector::live(7, pid, 9));
    inspector.term = super::recovery::SignalOutcome::Failed;
    journal.recover_orphans_with(&clock, &inspector).await;
    assert_eq!(abandoned_count(dir.path(), id), 0);
    assert_eq!(
        inspector.signals(),
        vec![(pid, super::recovery::Signal::Term)]
    );
}

#[tokio::test]
async fn a_failed_sigkill_never_abandons() {
    let dir = TempDir::new().unwrap();
    let id = Uuid::now_v7();
    let pid = 41_060;
    let journal = orphan_journal(dir.path(), id, pid, 7);
    let clock = SteppingClock::new(T0);
    let mut inspector = FakeInspector::new();
    inspector.set(pid, FakeInspector::live(7, pid, 9));
    inspector.deliver = false;
    inspector.kill = super::recovery::SignalOutcome::Failed;
    journal.recover_orphans_with(&clock, &inspector).await;
    assert_eq!(abandoned_count(dir.path(), id), 0);
    assert!(
        inspector
            .signals()
            .contains(&(pid, super::recovery::Signal::Kill))
    );
}

#[tokio::test]
async fn a_sigkill_that_leaves_the_group_alive_never_abandons() {
    let dir = TempDir::new().unwrap();
    let id = Uuid::now_v7();
    let pid = 41_070;
    let journal = orphan_journal(dir.path(), id, pid, 7);
    let clock = SteppingClock::new(T0);
    let mut inspector = FakeInspector::new();
    inspector.set(pid, FakeInspector::live(7, pid, 9));
    // SIGTERM and SIGKILL are both accepted by the kernel but never take effect.
    inspector.deliver = false;
    journal.recover_orphans_with(&clock, &inspector).await;
    assert_eq!(
        abandoned_count(dir.path(), id),
        0,
        "a successful killpg call is not proof the group is empty"
    );
}

#[tokio::test]
async fn a_reused_pid_is_never_signalled() {
    let dir = TempDir::new().unwrap();
    let id = Uuid::now_v7();
    let pid = 41_080;
    let journal = orphan_journal(dir.path(), id, pid, 7);
    let clock = SteppingClock::new(T0);
    let inspector = FakeInspector::new();
    // The PID now belongs to a different process.
    inspector.set(pid, FakeInspector::live(8, pid, 9));
    journal.recover_orphans_with(&clock, &inspector).await;
    assert!(inspector.signals().is_empty());
    assert_eq!(abandoned_count(dir.path(), id), 0);
}

#[tokio::test]
async fn an_unreadable_leader_is_left_unresolved() {
    let dir = TempDir::new().unwrap();
    let id = Uuid::now_v7();
    let pid = 41_090;
    let journal = orphan_journal(dir.path(), id, pid, 7);
    let clock = SteppingClock::new(T0);
    let inspector = FakeInspector::new();
    inspector.set(pid, super::recovery::ProcessLookup::Denied);
    journal.recover_orphans_with(&clock, &inspector).await;
    assert!(inspector.signals().is_empty());
    assert_eq!(abandoned_count(dir.path(), id), 0);
}

#[test]
fn a_failed_initial_boundary_leaves_no_adoptable_journal() {
    let dir = TempDir::new().unwrap();
    assert!(
        Journal::init_unlocked_with_faults(
            dir.path(),
            T0,
            BOOT,
            MONO,
            DurableFaults {
                rename: true,
                ..DurableFaults::default()
            },
        )
        .is_err()
    );
    assert!(
        Journal::open_unlocked(dir.path(), BOOT, MONO, limits()).is_err(),
        "an incomplete initialization is never opened"
    );
    let epoch = Journal::reset_unlocked(dir.path(), T0, BOOT, MONO).unwrap();
    assert!(!epoch.is_nil());
    assert!(Journal::open_unlocked(dir.path(), BOOT, MONO, limits()).is_ok());
}

#[test]
fn a_truncation_sync_failure_leaves_a_restart_safe_boundary() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let id = Uuid::now_v7();
    let committed;
    {
        let journal = open(dir.path());
        committed = committed_len(dir.path());
        journal.inject_append_fault(AppendFault::RecoverySyncTruncateSyncFail);
        assert!(
            journal
                .accept(&request(id, "container.restart", true), T0, BOOT, MONO)
                .is_err()
        );
        assert!(
            journal.corruption().is_some(),
            "an unconfirmed truncation stops further appends"
        );
    }
    assert_eq!(committed_len(dir.path()), committed);
    let reopened = open(dir.path());
    assert_eq!(state_of(&reopened, id), ActionState::NotFound);
    assert_eq!(journal_len(dir.path()), committed);
}

// ---- Commit-boundary confirmation -----------------------------------------

#[test]
fn startup_refuses_a_boundary_whose_file_sync_fails() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let err = Journal::open_unlocked_with_reconcile_faults(
        dir.path(),
        BOOT,
        MONO,
        limits(),
        super::commit::ReconcileFaults {
            file_sync: true,
            ..super::commit::ReconcileFaults::default()
        },
    );
    assert!(err.is_err(), "an unconfirmed boundary is never trusted");
}

#[test]
fn startup_refuses_a_boundary_whose_directory_sync_fails() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let err = Journal::open_unlocked_with_reconcile_faults(
        dir.path(),
        BOOT,
        MONO,
        limits(),
        super::commit::ReconcileFaults {
            dir_sync: true,
            ..super::commit::ReconcileFaults::default()
        },
    );
    assert!(err.is_err());
}

#[test]
fn a_boundary_that_differs_from_the_expectation_is_rejected() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let committed = committed_len(dir.path());
    assert!(
        super::commit::reconcile(dir.path(), committed + 1).is_err(),
        "the confirmed value must equal the intended value"
    );
    assert_eq!(committed_len(dir.path()), committed);
}

#[test]
fn a_visible_but_unconfirmed_boundary_is_confirmed_at_startup() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let committed = committed_len(dir.path());
    // The rename is visible but its directory entry was never synced.
    assert!(
        super::commit::write(
            dir.path(),
            committed,
            DurableFaults {
                dir_sync: true,
                ..DurableFaults::default()
            },
        )
        .is_err()
    );
    let journal = open(dir.path());
    assert!(journal.corruption().is_none());
    assert_eq!(state_of(&journal, Uuid::now_v7()), ActionState::NotFound);
}

// ---- Compaction transaction -----------------------------------------------

use super::compact::RewriteFaults;

fn frames(records: &[Record]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for record in records {
        bytes.extend_from_slice(&super::record::frame(record).unwrap());
    }
    bytes
}

fn write_compaction_intent(dir: &Path, epoch: Uuid, old: &[u8], new: &[u8]) {
    let intent = super::compaction::CompactionIntent {
        format: super::compaction::INTENT_FORMAT,
        transaction: Uuid::now_v7(),
        epoch,
        old_len: old.len() as u64,
        old_digest: super::compaction::digest(old),
        new_len: new.len() as u64,
        new_digest: super::compaction::digest(new),
    };
    super::compaction::write(dir, &intent, DurableFaults::default()).unwrap();
}

#[test]
fn a_shorter_journal_without_an_intent_is_never_adopted() {
    let dir = TempDir::new().unwrap();
    let epoch = init(dir.path());
    let kept = Uuid::now_v7();
    let lost = Uuid::now_v7();
    let journal = open(dir.path());
    journal
        .accept(&request(kept, "container.restart", true), T0, BOOT, MONO)
        .unwrap();
    journal.started(kept, 1, 1).unwrap();
    finished_execution(&journal, lost);
    drop(journal);
    let committed = committed_len(dir.path());
    // A complete, lifecycle-valid prefix of the committed journal.
    let prefix = frames(&[created(epoch), accepted(kept), started_rec(kept)]);
    assert!((prefix.len() as u64) < committed);
    std::fs::write(dir.path().join(JOURNAL_FILE), &prefix).unwrap();
    let reopened = open(dir.path());
    assert!(
        reopened.corruption().is_some(),
        "a shorter journal without a transaction is fenced"
    );
    assert_eq!(
        committed_len(dir.path()),
        committed,
        "the committed length is never silently lowered"
    );
    assert_eq!(
        journal_len(dir.path()),
        prefix.len() as u64,
        "the shorter file is left exactly as found; it is never repaired implicitly"
    );
    assert!(matches!(
        reopened.accept(
            &request(Uuid::now_v7(), "container.restart", true),
            T0,
            BOOT,
            MONO
        ),
        Ok(AcceptOutcome::Refused(_))
    ));
}

#[test]
fn a_shorter_journal_with_a_wrong_digest_intent_is_refused() {
    let dir = TempDir::new().unwrap();
    let epoch = init(dir.path());
    let kept = Uuid::now_v7();
    let journal = open(dir.path());
    journal
        .accept(&request(kept, "container.restart", true), T0, BOOT, MONO)
        .unwrap();
    journal.started(kept, 1, 1).unwrap();
    drop(journal);
    let full = std::fs::read(dir.path().join(JOURNAL_FILE)).unwrap();
    // A shorter, complete journal whose length matches the intent but whose digest does not.
    let prefix = frames(&[created(epoch)]);
    let intent = super::compaction::CompactionIntent {
        format: super::compaction::INTENT_FORMAT,
        transaction: Uuid::now_v7(),
        epoch,
        old_len: full.len() as u64,
        old_digest: super::compaction::digest(&full),
        new_len: prefix.len() as u64,
        new_digest: super::compaction::digest(b"a different compacted journal"),
    };
    super::compaction::write(dir.path(), &intent, DurableFaults::default()).unwrap();
    std::fs::write(dir.path().join(JOURNAL_FILE), &prefix).unwrap();
    assert!(
        Journal::open_unlocked(dir.path(), BOOT, MONO, limits()).is_err(),
        "a length match without the digest is not proof"
    );
    assert!(super::compaction::exists(dir.path()), "the intent is kept");
}

#[test]
fn a_compaction_intent_for_another_epoch_is_refused() {
    let dir = TempDir::new().unwrap();
    let epoch = init(dir.path());
    let full = std::fs::read(dir.path().join(JOURNAL_FILE)).unwrap();
    write_compaction_intent(dir.path(), Uuid::now_v7(), &full, &full[..full.len()]);
    assert!(Journal::open_unlocked(dir.path(), BOOT, MONO, limits()).is_err());
    let _ = epoch;
}

#[test]
fn a_legitimate_compaction_is_recovered_after_the_boundary_update_is_lost() {
    let dir = TempDir::new().unwrap();
    let epoch = init(dir.path());
    let unresolved = Uuid::now_v7();
    let finished = Uuid::now_v7();
    {
        let journal = open(dir.path());
        journal
            .accept(
                &request(unresolved, "container.restart", true),
                T0,
                BOOT,
                MONO,
            )
            .unwrap();
        finished_execution(&journal, finished);
    }
    let full = std::fs::read(dir.path().join(JOURNAL_FILE)).unwrap();
    let compacted = frames(&[created(epoch), accepted(unresolved)]);
    write_compaction_intent(dir.path(), epoch, &full, &compacted);
    std::fs::write(dir.path().join(JOURNAL_FILE), &compacted).unwrap();
    // The boundary still names the old, longer journal.
    assert_eq!(committed_len(dir.path()), full.len() as u64);

    let reopened = open(dir.path());
    assert!(reopened.corruption().is_none());
    assert!(!super::compaction::exists(dir.path()));
    assert_eq!(committed_len(dir.path()), compacted.len() as u64);
    assert_eq!(journal_len(dir.path()), compacted.len() as u64);
    assert!(matches!(
        state_of(&reopened, unresolved),
        ActionState::OutcomeUnknown {
            reason
        } if reason.known() == Some(&OutcomeUnknownReason::NoStartRecord)
    ));
    assert!(!matches!(
        state_of(&reopened, finished),
        ActionState::Finished { .. }
    ));
}

#[test]
fn an_interrupted_compaction_keeps_the_original_journal() {
    let dir = TempDir::new().unwrap();
    let epoch = init(dir.path());
    let finished = Uuid::now_v7();
    {
        let journal = open(dir.path());
        finished_execution(&journal, finished);
    }
    let full = std::fs::read(dir.path().join(JOURNAL_FILE)).unwrap();
    let compacted = frames(&[created(epoch)]);
    write_compaction_intent(dir.path(), epoch, &full, &compacted);
    std::fs::write(super::compaction::temp_path(dir.path()), &compacted).unwrap();

    let reopened = open(dir.path());
    assert!(reopened.corruption().is_none());
    assert!(!super::compaction::exists(dir.path()));
    assert!(!super::compaction::temp_path(dir.path()).exists());
    assert_eq!(committed_len(dir.path()), full.len() as u64);
    assert!(
        matches!(state_of(&reopened, finished), ActionState::Finished { .. }),
        "committed records are not discarded by an aborted transaction"
    );
}

#[test]
fn a_completed_but_uncleaned_compaction_is_only_cleaned_up() {
    let dir = TempDir::new().unwrap();
    let epoch = init(dir.path());
    let finished = Uuid::now_v7();
    {
        let journal = open(dir.path());
        finished_execution(&journal, finished);
    }
    let full = std::fs::read(dir.path().join(JOURNAL_FILE)).unwrap();
    let compacted = frames(&[created(epoch)]);
    write_compaction_intent(dir.path(), epoch, &full, &compacted);
    std::fs::write(dir.path().join(JOURNAL_FILE), &compacted).unwrap();
    super::commit::write(dir.path(), compacted.len() as u64, DurableFaults::default()).unwrap();

    let reopened = open(dir.path());
    assert!(reopened.corruption().is_none());
    assert!(!super::compaction::exists(dir.path()));
    assert_eq!(journal_len(dir.path()), compacted.len() as u64);
}

#[test]
fn reset_archives_an_interrupted_compaction_and_leaves_no_intent() {
    let dir = TempDir::new().unwrap();
    let epoch = init(dir.path());
    let finished = Uuid::now_v7();
    {
        let journal = open(dir.path());
        finished_execution(&journal, finished);
    }
    let full = std::fs::read(dir.path().join(JOURNAL_FILE)).unwrap();
    let compacted = frames(&[created(epoch)]);
    write_compaction_intent(dir.path(), epoch, &full, &compacted);
    std::fs::write(dir.path().join(JOURNAL_FILE), &compacted).unwrap();

    let new_epoch = Journal::reset_unlocked(dir.path(), T0, BOOT, MONO).unwrap();
    assert_ne!(new_epoch, epoch);
    assert!(!super::compaction::exists(dir.path()));
    assert!(
        dir.path()
            .join(format!("{}.{epoch}", super::compaction::INTENT_FILE))
            .exists(),
        "the transaction is preserved in the archive"
    );
    assert_eq!(
        std::fs::read(dir.path().join(format!("{JOURNAL_FILE}.{epoch}"))).unwrap(),
        compacted
    );
    let reopened = open(dir.path());
    assert!(reopened.corruption().is_none());
    assert_eq!(reopened.epoch(), new_epoch);
}

#[test]
fn every_compaction_stage_is_recoverable() {
    #[derive(Clone, Copy, PartialEq)]
    enum Expected {
        Aborted,
        Confirmed,
        Fenced,
    }
    use Expected::{Aborted, Confirmed, Fenced};

    let cases: &[(&str, RewriteFaults, Expected)] = &[
        (
            "intent temporary file",
            RewriteFaults {
                intent_temp_sync: true,
                ..RewriteFaults::default()
            },
            Aborted,
        ),
        (
            "intent rename",
            RewriteFaults {
                intent_rename: true,
                ..RewriteFaults::default()
            },
            Aborted,
        ),
        (
            "intent directory sync",
            RewriteFaults {
                intent_dir_sync: true,
                ..RewriteFaults::default()
            },
            Aborted,
        ),
        (
            "compacted temporary write",
            RewriteFaults {
                temp_write: true,
                ..RewriteFaults::default()
            },
            Aborted,
        ),
        (
            "compacted temporary sync",
            RewriteFaults {
                temp_sync: true,
                ..RewriteFaults::default()
            },
            Aborted,
        ),
        (
            "journal rename",
            RewriteFaults {
                rename: true,
                ..RewriteFaults::default()
            },
            Aborted,
        ),
        (
            "journal directory sync",
            RewriteFaults {
                journal_dir_sync: true,
                ..RewriteFaults::default()
            },
            Confirmed,
        ),
        (
            "journal directory sync and recovery",
            RewriteFaults {
                journal_dir_sync: true,
                journal_dir_recovery_fails: true,
                ..RewriteFaults::default()
            },
            Fenced,
        ),
        (
            "boundary temporary file",
            RewriteFaults {
                boundary_temp_sync: true,
                ..RewriteFaults::default()
            },
            Confirmed,
        ),
        (
            "boundary rename",
            RewriteFaults {
                boundary_rename: true,
                ..RewriteFaults::default()
            },
            Confirmed,
        ),
        (
            "boundary directory sync",
            RewriteFaults {
                boundary_dir_sync: true,
                ..RewriteFaults::default()
            },
            Confirmed,
        ),
        (
            "boundary confirmation",
            RewriteFaults {
                boundary_reconcile_fails: true,
                ..RewriteFaults::default()
            },
            Fenced,
        ),
        (
            "boundary directory sync and confirmation",
            RewriteFaults {
                boundary_dir_sync: true,
                boundary_reconcile_fails: true,
                ..RewriteFaults::default()
            },
            Fenced,
        ),
        (
            "intent removal",
            RewriteFaults {
                intent_remove: true,
                ..RewriteFaults::default()
            },
            Fenced,
        ),
        (
            "intent removal directory sync",
            RewriteFaults {
                intent_remove_dir_sync: true,
                ..RewriteFaults::default()
            },
            Fenced,
        ),
    ];

    for (stage, faults, expected) in cases {
        let dir = TempDir::new().unwrap();
        init(dir.path());
        let unresolved = Uuid::now_v7();
        let finished = Uuid::now_v7();
        {
            let journal = open(dir.path());
            journal
                .accept(
                    &request(unresolved, "container.restart", true),
                    T0,
                    BOOT,
                    MONO,
                )
                .unwrap();
            finished_execution(&journal, finished);
            journal.inject_rewrite_faults(*faults);
            let removed = journal
                .prune(0, MONO + super::limiter::WINDOW_NS + 1, BOOT)
                .unwrap();
            match expected {
                Aborted => {
                    assert!(!removed, "{stage}");
                    assert!(journal.corruption().is_none(), "{stage}");
                    assert!(
                        matches!(state_of(&journal, finished), ActionState::Finished { .. }),
                        "{stage}: the aborted compaction keeps history"
                    );
                }
                Confirmed => {
                    assert!(removed, "{stage}");
                    assert!(journal.corruption().is_none(), "{stage}");
                }
                Fenced => {
                    assert!(!removed, "{stage}");
                    assert!(journal.corruption().is_some(), "{stage}");
                    assert!(
                        matches!(
                            journal.accept(
                                &request(Uuid::now_v7(), "container.restart", true),
                                T0,
                                BOOT,
                                MONO
                            ),
                            Ok(AcceptOutcome::Refused(_))
                        ),
                        "{stage}: an unconfirmed state is not writable"
                    );
                }
            }
        }
        let reopened = open(dir.path());
        assert!(
            reopened.corruption().is_none(),
            "{stage}: restart resolves it"
        );
        assert!(
            !super::compaction::exists(dir.path()),
            "{stage}: no intent is left"
        );
        assert_eq!(
            committed_len(dir.path()),
            journal_len(dir.path()),
            "{stage}: the boundary names the journal"
        );
        assert!(
            matches!(
                state_of(&reopened, unresolved),
                ActionState::OutcomeUnknown { reason }
                    if reason.known() == Some(&OutcomeUnknownReason::NoStartRecord)
            ),
            "{stage}: the unresolved execution is kept"
        );
        match expected {
            Aborted => assert!(
                matches!(state_of(&reopened, finished), ActionState::Finished { .. }),
                "{stage}: history survives the abort"
            ),
            _ => assert!(
                !matches!(state_of(&reopened, finished), ActionState::Finished { .. }),
                "{stage}: the compaction result is adopted"
            ),
        }
        // Repeated recovery is idempotent.
        let again = open(dir.path());
        assert_eq!(again.epoch(), reopened.epoch(), "{stage}");
        assert_eq!(
            committed_len(dir.path()),
            journal_len(dir.path()),
            "{stage}"
        );
        assert!(!super::compaction::exists(dir.path()), "{stage}");
    }
}

#[test]
fn compaction_keeps_mutating_acceptances_inside_the_window() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let now = 10 * super::limiter::WINDOW_NS;
    let inside = Uuid::now_v7();
    let outside = Uuid::now_v7();
    {
        let journal = open(dir.path());
        for (id, mono) in [
            (inside, now - super::limiter::WINDOW_NS / 2),
            (outside, now - 2 * super::limiter::WINDOW_NS),
        ] {
            journal
                .accept(&request(id, "container.restart", true), T0, BOOT, mono)
                .unwrap();
            journal.started(id, 1, 1).unwrap();
            journal.finished(id, result()).unwrap();
        }
        assert!(
            journal.prune(0, now, BOOT).unwrap(),
            "the expired execution is removed"
        );
        assert!(matches!(
            state_of(&journal, inside),
            ActionState::Finished { .. }
        ));
        assert!(!matches!(
            state_of(&journal, outside),
            ActionState::Finished { .. }
        ));
    }
    let reopened = open(dir.path());
    assert!(reopened.corruption().is_none());
    assert!(
        matches!(state_of(&reopened, inside), ActionState::Finished { .. }),
        "the rate limiter's window survives compaction and restart"
    );
}

#[test]
fn init_refuses_a_leftover_compaction_intent() {
    let dir = TempDir::new().unwrap();
    write_compaction_intent(dir.path(), Uuid::now_v7(), b"old", b"new");
    assert!(Journal::init_unlocked(dir.path(), T0, BOOT, MONO).is_err());
    assert!(super::compaction::exists(dir.path()));
}

// ---- Compaction intent cleanup safety -------------------------------------

fn seeded_compaction_history(dir: &Path) -> (Uuid, Uuid) {
    let journal = open(dir);
    let unresolved = Uuid::now_v7();
    let finished = Uuid::now_v7();
    journal
        .accept(
            &request(unresolved, "container.restart", true),
            T0,
            BOOT,
            MONO,
        )
        .unwrap();
    finished_execution(&journal, finished);
    (unresolved, finished)
}

fn assert_mutations_refused(journal: &Journal, accepted: Uuid) {
    assert!(matches!(
        journal.accept(
            &request(Uuid::now_v7(), "container.restart", true),
            T0,
            BOOT,
            MONO
        ),
        Ok(AcceptOutcome::Refused(_))
    ));
    assert!(journal.started(accepted, 1, 1).is_err());
    assert!(journal.finished(accepted, result()).is_err());
    assert!(
        journal
            .prune(0, MONO + super::limiter::WINDOW_NS + 1, BOOT)
            .is_err()
    );
}

#[test]
fn a_completed_compaction_with_failed_intent_removal_fences_until_restart() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let (unresolved, finished) = seeded_compaction_history(dir.path());
    let journal = open(dir.path());
    journal.inject_rewrite_faults(RewriteFaults {
        intent_remove: true,
        ..RewriteFaults::default()
    });
    assert!(
        !journal
            .prune(0, MONO + super::limiter::WINDOW_NS + 1, BOOT)
            .unwrap(),
        "an unconfirmed cleanup is not a confirmed prune"
    );
    assert!(journal.corruption().is_some(), "mutations must stop");
    assert!(super::compaction::exists(dir.path()));
    assert_eq!(committed_len(dir.path()), journal_len(dir.path()));
    assert_mutations_refused(&journal, unresolved);

    // Startup must resolve the surviving intent before any mutation is possible.
    drop(journal);
    let reopened = open(dir.path());
    assert!(reopened.corruption().is_none());
    assert!(!super::compaction::exists(dir.path()));
    assert_eq!(committed_len(dir.path()), journal_len(dir.path()));
    assert!(matches!(
        state_of(&reopened, unresolved),
        ActionState::OutcomeUnknown {
            reason
        } if reason.known() == Some(&OutcomeUnknownReason::NoStartRecord)
    ));
    assert!(!matches!(
        state_of(&reopened, finished),
        ActionState::Finished { .. }
    ));
    assert_eq!(
        reopened
            .accept(
                &request(Uuid::now_v7(), "container.restart", true),
                T0,
                BOOT,
                MONO
            )
            .unwrap(),
        AcceptOutcome::Accepted,
        "mutations resume after the transaction is resolved"
    );
    let again = open(dir.path());
    assert!(again.corruption().is_none());
    assert!(!super::compaction::exists(dir.path()));
}

#[test]
fn an_unsynced_intent_removal_fences_and_recovers_when_the_intent_reappears() {
    let dir = TempDir::new().unwrap();
    let epoch = init(dir.path());
    let (unresolved, finished) = seeded_compaction_history(dir.path());
    let old = std::fs::read(dir.path().join(JOURNAL_FILE)).unwrap();
    let journal = open(dir.path());
    journal.inject_rewrite_faults(RewriteFaults {
        intent_remove_dir_sync: true,
        ..RewriteFaults::default()
    });
    assert!(
        !journal
            .prune(0, MONO + super::limiter::WINDOW_NS + 1, BOOT)
            .unwrap()
    );
    assert!(
        journal.corruption().is_some(),
        "an unsynced deletion is uncertainty, not cleanup"
    );
    assert!(
        !super::compaction::exists(dir.path()),
        "the current view lost the name, but that is not proof"
    );
    assert_mutations_refused(&journal, unresolved);
    let new = std::fs::read(dir.path().join(JOURNAL_FILE)).unwrap();
    drop(journal);

    // The crash: an unsynced deletion did not survive, so the intent name reappears.
    write_compaction_intent(dir.path(), epoch, &old, &new);
    let reopened = open(dir.path());
    assert!(reopened.corruption().is_none());
    assert!(!super::compaction::exists(dir.path()));
    assert_eq!(committed_len(dir.path()), journal_len(dir.path()));
    assert!(!matches!(
        state_of(&reopened, finished),
        ActionState::Finished { .. }
    ));
    assert_eq!(
        reopened
            .accept(
                &request(Uuid::now_v7(), "container.restart", true),
                T0,
                BOOT,
                MONO
            )
            .unwrap(),
        AcceptOutcome::Accepted
    );
    let again = open(dir.path());
    assert!(again.corruption().is_none());
    assert_eq!(committed_len(dir.path()), journal_len(dir.path()));
}

#[test]
fn an_aborted_compaction_with_failed_intent_cleanup_fences_and_aborts_at_restart() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let (unresolved, finished) = seeded_compaction_history(dir.path());
    let old = std::fs::read(dir.path().join(JOURNAL_FILE)).unwrap();
    let journal = open(dir.path());
    journal.inject_rewrite_faults(RewriteFaults {
        temp_write: true,
        intent_remove: true,
        ..RewriteFaults::default()
    });
    assert!(
        !journal
            .prune(0, MONO + super::limiter::WINDOW_NS + 1, BOOT)
            .unwrap()
    );
    assert!(journal.corruption().is_some());
    assert_eq!(
        std::fs::read(dir.path().join(JOURNAL_FILE)).unwrap(),
        old,
        "the original journal is untouched"
    );
    assert!(
        matches!(state_of(&journal, finished), ActionState::Finished { .. }),
        "the in-memory state is the original, not the aborted compaction"
    );
    assert_mutations_refused(&journal, unresolved);
    assert!(super::compaction::exists(dir.path()));

    drop(journal);
    let reopened = open(dir.path());
    assert!(reopened.corruption().is_none());
    assert!(!super::compaction::exists(dir.path()));
    assert_eq!(
        std::fs::read(dir.path().join(JOURNAL_FILE)).unwrap(),
        old,
        "every original record is recovered"
    );
    assert!(matches!(
        state_of(&reopened, finished),
        ActionState::Finished { .. }
    ));
    assert_eq!(
        reopened
            .accept(
                &request(Uuid::now_v7(), "container.restart", true),
                T0,
                BOOT,
                MONO
            )
            .unwrap(),
        AcceptOutcome::Accepted
    );
}

#[test]
fn an_indeterminate_intent_write_with_failed_cleanup_fences_until_restart() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let (unresolved, finished) = seeded_compaction_history(dir.path());
    let old = std::fs::read(dir.path().join(JOURNAL_FILE)).unwrap();
    let journal = open(dir.path());
    journal.inject_rewrite_faults(RewriteFaults {
        intent_dir_sync: true,
        intent_remove: true,
        ..RewriteFaults::default()
    });
    assert!(
        !journal
            .prune(0, MONO + super::limiter::WINDOW_NS + 1, BOOT)
            .unwrap()
    );
    assert!(
        journal.corruption().is_some(),
        "a possibly surviving intent blocks mutations"
    );
    assert!(super::compaction::exists(dir.path()));
    assert_eq!(std::fs::read(dir.path().join(JOURNAL_FILE)).unwrap(), old);
    assert_mutations_refused(&journal, unresolved);

    drop(journal);
    let reopened = open(dir.path());
    assert!(reopened.corruption().is_none());
    assert!(!super::compaction::exists(dir.path()));
    assert_eq!(std::fs::read(dir.path().join(JOURNAL_FILE)).unwrap(), old);
    assert!(matches!(
        state_of(&reopened, finished),
        ActionState::Finished { .. }
    ));
    assert_eq!(
        reopened
            .accept(
                &request(Uuid::now_v7(), "container.restart", true),
                T0,
                BOOT,
                MONO
            )
            .unwrap(),
        AcceptOutcome::Accepted
    );
}

#[test]
fn confirmed_cleanup_leaves_the_running_journal_writable() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let (unresolved, finished) = seeded_compaction_history(dir.path());
    let journal = open(dir.path());
    assert!(
        journal
            .prune(0, MONO + super::limiter::WINDOW_NS + 1, BOOT)
            .unwrap(),
        "an ordinary compaction is confirmed"
    );
    assert!(journal.corruption().is_none());
    assert!(!super::compaction::exists(dir.path()));
    assert!(!matches!(
        state_of(&journal, finished),
        ActionState::Finished { .. }
    ));
    assert!(matches!(
        journal
            .accept(
                &request(unresolved, "container.restart", true),
                T0,
                BOOT,
                MONO
            )
            .unwrap(),
        AcceptOutcome::Duplicate(_)
    ));
    let fresh = Uuid::now_v7();
    assert_eq!(
        journal
            .accept(&request(fresh, "container.restart", true), T0, BOOT, MONO)
            .unwrap(),
        AcceptOutcome::Accepted
    );
}

#[test]
fn an_aborted_compaction_with_confirmed_cleanup_stays_writable() {
    let dir = TempDir::new().unwrap();
    init(dir.path());
    let (unresolved, finished) = seeded_compaction_history(dir.path());
    let journal = open(dir.path());
    journal.inject_rewrite_faults(RewriteFaults {
        temp_write: true,
        ..RewriteFaults::default()
    });
    assert!(
        !journal
            .prune(0, MONO + super::limiter::WINDOW_NS + 1, BOOT)
            .unwrap()
    );
    assert!(journal.corruption().is_none());
    assert!(!super::compaction::exists(dir.path()));
    assert!(matches!(
        state_of(&journal, finished),
        ActionState::Finished { .. }
    ));
    assert!(journal.started(unresolved, 1, 1).is_ok());
}
