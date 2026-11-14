//! Contract for [`AdmissionRepo`].

use domovoi_core::ports::{AdmissionOutcome, AdmissionPurpose, AdmissionRepo};
use jiff::{SignedDuration, Timestamp};

use super::ContractResult;
use crate::build::at;

/// Exercises the rolling window, its transactional check-and-insert and its pruning.
pub async fn admission_repo_contract(repo: &dyn AdmissionRepo) -> ContractResult {
    const WINDOW: SignedDuration = SignedDuration::from_hours(1);
    let first = at(10_000);
    let second = at(10_060);
    let third = at(10_120);

    assert_eq!(repo.starts_since(at(0)).await?, Vec::<Timestamp>::new());

    assert_eq!(
        repo.admit(AdmissionPurpose::Triage, 2, WINDOW, first)
            .await?,
        AdmissionOutcome::Admitted
    );
    assert_eq!(
        repo.admit(AdmissionPurpose::Question, 2, WINDOW, second)
            .await?,
        AdmissionOutcome::Admitted
    );
    assert_eq!(
        repo.admit(AdmissionPurpose::Triage, 2, WINDOW, third)
            .await?,
        AdmissionOutcome::Full {
            starts: vec![first, second]
        },
        "room is spent, and the recorded starts come back oldest first"
    );

    // A zero room never admits and never inserts.
    assert_eq!(
        repo.admit(AdmissionPurpose::Question, 0, WINDOW, third)
            .await?,
        AdmissionOutcome::Full {
            starts: vec![first, second]
        },
    );
    assert_eq!(repo.starts_since(at(0)).await?, vec![first, second]);

    // A start older than the window is not counted towards the room.
    let past_window = at(10_000 + 3_601);
    assert_eq!(
        repo.admit(AdmissionPurpose::Triage, 2, WINDOW, past_window)
            .await?,
        AdmissionOutcome::Admitted,
    );

    // Rows older than twice the window are pruned, so they no longer show in a far read.
    let past_two_windows = at(10_000 + 7_201);
    assert_eq!(
        repo.admit(AdmissionPurpose::Triage, 10, WINDOW, past_two_windows)
            .await?,
        AdmissionOutcome::Admitted,
    );
    assert_eq!(
        repo.starts_since(at(0)).await?,
        vec![second, past_window, past_two_windows],
        "the pruned start is gone and the rest stay oldest first"
    );
    Ok(())
}
