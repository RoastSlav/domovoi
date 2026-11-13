//! R-6 benchmark: what `synchronous=FULL` costs per commit. Not a gate.

use std::time::Instant;

use domovoi_core::ports::{
    Expected, IncidentRepo, IncidentTimelineEntry, StateWrite, Transaction, UnitOfWork,
};
use domovoi_store::{Store, StoreOptions};
use domovoi_testkit::build::{T0, at, host, open_incident};

fn report(label: &str, samples: &mut [f64]) {
    samples.sort_by(f64::total_cmp);
    let median = samples[samples.len() / 2];
    let p95 = samples[(samples.len() * 95 / 100).min(samples.len() - 1)];
    let max = samples[samples.len() - 1];
    eprintln!(
        "{label}: n={} median={median:.3}ms p95={p95:.3}ms max={max:.3}ms",
        samples.len()
    );
}

#[tokio::test]
#[ignore = "benchmark: run with --ignored --nocapture"]
async fn commit_latency_with_full_synchronous() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db").join("domovoi.db");
    let store = Store::open(&path, StoreOptions::beside(&path, T0))
        .await
        .unwrap();
    let host = host("bench").unwrap();

    let mut commit_samples = Vec::new();
    for n in 0..300i64 {
        let incident = open_incident(host.id, &format!("bench.{n}"), None).unwrap();
        let entry = IncidentTimelineEntry {
            incident: incident.id(),
            at: at(n),
            kind: "status.changed".to_owned(),
            summary: "opened".to_owned(),
            detail: serde_json::Value::Null,
        };
        let tx = Transaction::new()
            .write(StateWrite::Incident {
                incident,
                expected: Expected::New,
            })
            .write(StateWrite::IncidentEvent(entry));
        let started = Instant::now();
        store.commit(tx).await.unwrap();
        commit_samples.push(started.elapsed().as_secs_f64() * 1_000.0);
    }
    report("commit", &mut commit_samples);

    let mut insert_samples = Vec::new();
    for n in 0..300i64 {
        let incident = open_incident(host.id, &format!("repo.{n}"), None).unwrap();
        let started = Instant::now();
        IncidentRepo::insert(&store, &incident).await.unwrap();
        insert_samples.push(started.elapsed().as_secs_f64() * 1_000.0);
    }
    report("repository insert", &mut insert_samples);
}
