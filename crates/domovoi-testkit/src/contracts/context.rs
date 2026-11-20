//! The contract every context source keeps: bounded, redacted, unknown never empty, nothing
//! after the deadline, and a stable identity for the same fact.

use domovoi_core::context::{ContextQuery, ContextScope, ContextSelector, ItemStatus};
use domovoi_core::ports::{Clock, ContextSource};
use jiff::SignedDuration;

use crate::clock::FakeClock;

/// What the suite needs to drive one source through each obligation: the source, a selector
/// it supports, the clock it reads, the secret its redactor knows, and a way to make its next
/// answer carry some text or fail.
pub trait ContextFixture: Send + Sync {
    /// The source under test.
    fn source(&self) -> &dyn ContextSource;
    /// A selector the source supports.
    fn selector(&self) -> ContextSelector;
    /// The fake clock the source reads.
    fn clock(&self) -> &FakeClock;
    /// A value the source's redactor removes.
    fn secret(&self) -> &str;
    /// Makes the next answers for [`selector`](Self::selector) carry `text`.
    fn answer_with(&self, text: &str);
    /// Makes the next answers for [`selector`](Self::selector) fail.
    fn fail_next(&self);
}

fn query(fixture: &dyn ContextFixture, scope: &ContextScope, max_item_bytes: u32) -> ContextQuery {
    ContextQuery {
        scope: scope.clone(),
        selector: fixture.selector(),
        limit: 1,
        max_item_bytes,
        deadline: fixture.clock().now() + SignedDuration::from_secs(30),
    }
}

/// Runs every testable obligation of the `ContextSource` port against `fixture` in `scope`.
pub async fn run(fixture: &dyn ContextFixture, scope: ContextScope) {
    let source = fixture.source();

    // Bounded and redacted: a long answer that quotes the secret.
    let long = format!("{} {}", fixture.secret(), "x".repeat(4096));
    fixture.answer_with(&long);
    assert!(
        source.supports(&fixture.selector()),
        "the fixture's selector is supported"
    );
    let bounded = query(fixture, &scope, 256);
    let batch = source.fetch(&bounded).await;
    assert!(
        batch.items.len() <= usize::from(bounded.limit),
        "at most `limit` items: {}",
        batch.items.len()
    );
    assert!(!batch.items.is_empty(), "a known answer is an item");
    for item in &batch.items {
        assert!(item.body.len() <= 256, "a body within max_item_bytes");
        assert!(item.truncated, "a cut body says it was cut");
        assert!(
            (1..=120).contains(&item.title.chars().count()),
            "a title of 1 to 120 characters"
        );
        assert!(item.provenance.reference.chars().count() <= 120);
        let shown = format!(
            "{}{}{}{:?}",
            item.title, item.body, item.provenance.reference, item.provenance.actor
        );
        assert!(
            !shown.contains(fixture.secret()),
            "the secret reached an item"
        );
        assert_eq!(item.source, source.id(), "an item names its source");
        assert_eq!(
            item.layer,
            source.layer(),
            "an item sits in its source's layer"
        );
    }

    // Stable identity: the same fact twice has the same id.
    let first = source.fetch(&query(fixture, &scope, 1024)).await;
    let again = source.fetch(&query(fixture, &scope, 1024)).await;
    let ids = |batch: &domovoi_core::context::ContextBatch| -> Vec<String> {
        batch
            .items
            .iter()
            .map(|item| item.id.as_str().to_owned())
            .collect()
    };
    assert_eq!(ids(&first), ids(&again), "the same fact keeps its id");

    // Unknown is not empty: a failure is an unknown item or a gap.
    fixture.fail_next();
    let failed = source.fetch(&query(fixture, &scope, 1024)).await;
    let unknown = failed
        .items
        .iter()
        .any(|item| matches!(item.status, ItemStatus::Unknown { .. }));
    assert!(
        unknown || !failed.gaps.is_empty(),
        "a failed read is never an empty batch: {failed:?}"
    );
    for gap in &failed.gaps {
        assert!(
            gap.reason.chars().count() <= 200,
            "a gap reason of at most 200 characters"
        );
        assert!(
            !gap.reason.contains(fixture.secret()),
            "the secret reached a gap"
        );
    }

    // Deadline: nothing is answered after it.
    fixture.answer_with("late");
    let mut late = query(fixture, &scope, 1024);
    late.deadline = fixture.clock().now();
    let batch = source.fetch(&late).await;
    assert!(batch.items.is_empty(), "no item past the deadline");
    assert!(
        batch.gaps.iter().any(|gap| gap.reason == "timed out"),
        "past the deadline the answer is a `timed out` gap: {batch:?}"
    );
}
