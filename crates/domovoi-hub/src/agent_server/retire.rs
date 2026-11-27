//! Marking facts as no longer reported when a digest arrives.
//!
//! An agent names the keys a check dropped while it ran (`retired`). It cannot name a key it
//! dropped before it last started, so the hub also compares the digest with what it has
//! stored. A check replaces all its facts in one run, so a stored key is gone when the digest
//! carries newer facts of the same shape (`disk.<target>.free_gib`) but not that key. A shape
//! the digest does not carry at all proves nothing: its check may have failed since the agent
//! started, and its facts stay as stale.

use std::collections::{BTreeMap, BTreeSet};

use domovoi_core::{DigestFact, HostId};
use domovoi_proto::messages::DigestFacts;
use jiff::{SignedDuration, Timestamp};

use super::Inner;
use crate::api::hosts::FACT_WINDOW;

/// The first and last part of a key that names a target in between, such as `disk` and
/// `free_gib` of `disk./boot.free_gib`.
fn shape(key: &str) -> Option<(&str, &str)> {
    let (family, rest) = key.split_once('.')?;
    let (_, field) = rest.rsplit_once('.')?;
    Some((family, field))
}

/// The keys of `stored` that `digest` shows are gone.
fn vanished<'a>(stored: &'a [DigestFact], digest: &[&DigestFact]) -> BTreeSet<&'a str> {
    let sent: BTreeSet<&str> = digest.iter().map(|f| f.key.as_str()).collect();
    let mut newest: BTreeMap<(&str, &str), Timestamp> = BTreeMap::new();
    for fact in digest {
        if let Some(shape) = shape(&fact.key) {
            let at = newest.entry(shape).or_insert(fact.observed_at);
            *at = (*at).max(fact.observed_at);
        }
    }
    stored
        .iter()
        .filter(|f| !sent.contains(f.key.as_str()))
        .filter(|f| {
            shape(&f.key)
                .and_then(|shape| newest.get(&shape))
                .is_some_and(|newer| f.observed_at < *newer)
        })
        .map(|f| f.key.as_str())
        .collect()
}

async fn retire(inner: &Inner, host: HostId, key: &str, at: Timestamp) {
    if let Err(err) = inner.deps.facts.retire(host, key, at).await {
        tracing::warn!(%host, %err, key, "could not retire a fact");
    }
}

/// Records what `digest` says, and shows, is no longer reported for `host`. The samples stay
/// as history; a new sample of a key makes it current again.
pub(super) async fn apply(inner: &Inner, host: HostId, digest: &DigestFacts) {
    let now = inner.deps.clock.now();
    for key in &digest.retired {
        retire(inner, host, key, now).await;
    }
    let since = now.checked_sub(FACT_WINDOW).unwrap_or(now);
    let until = now.checked_add(SignedDuration::from_secs(1)).unwrap_or(now);
    let facts = &inner.deps.facts;
    let (stored, marks) = match (
        facts.list(host, since, until).await,
        facts.retired(host).await,
    ) {
        (Ok(stored), Ok(marks)) => (stored, marks),
        (Err(err), _) | (_, Err(err)) => {
            tracing::warn!(%host, %err, "could not compare a digest with the stored facts");
            return;
        }
    };
    let ours: Vec<&DigestFact> = digest.facts.iter().filter(|f| f.host == host).collect();
    for key in vanished(&stored, &ours) {
        // A key marked since its last sample keeps the time it was first marked.
        let marked = marks.iter().any(|(marked, at)| {
            marked == key && stored.iter().all(|f| f.key != key || f.observed_at <= *at)
        });
        if !marked {
            retire(inner, host, key, now).await;
        }
    }
}
