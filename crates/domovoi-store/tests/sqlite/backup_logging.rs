//! The backup verification failure path must not log a path or error before the caller
//! redacts it: the redactor lives in the hub, above this crate.

use std::io::Write;
use std::sync::{Arc, Mutex};

use domovoi_store::{Depth, verify_backup, verify_or_track_for_tests, verify_or_tracked_for_tests};
use domovoi_testkit::build::{T0, at};

use tracing_subscriber::fmt::MakeWriter;

use crate::common::Fresh;

/// A secret-shaped name, so a leak is unmistakable in the captured output.
const SECRET: &str = "dispatch-webhook-secret-1a2b3c";

/// Collects every log line written while it is installed.
#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<u8>>>);

impl Write for Lines {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Lines {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

impl Lines {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

/// A database file at a path containing the secret, already damaged.
async fn damaged_backup_at(dir: &std::path::Path, secret: &str) -> std::path::PathBuf {
    let fresh = Fresh::new().await;
    let good = fresh.store.backup_to(dir, at(0)).await.unwrap();
    fresh.store.close().await;

    let folder = dir.join(format!("backups-{secret}"));
    std::fs::create_dir_all(&folder).unwrap();
    let damaged = folder.join(format!("domovoi-{secret}.db"));
    let mut bytes = std::fs::read(&good).unwrap();
    // Break SQLite's own page structure so verification fails.
    let page = 4096;
    for slot in 2..bytes.len() / page {
        let start = slot * page;
        for (offset, byte) in bytes[start..start + page].iter_mut().enumerate() {
            *byte = (offset % 251) as u8;
        }
    }
    std::fs::write(&damaged, &bytes).unwrap();
    damaged
}

#[tokio::test]
async fn a_failed_verification_logs_no_path_or_error_before_redaction() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join(format!("backups-{SECRET}"));
    std::fs::create_dir_all(&folder).unwrap();
    // A path error carries the path in its message, so a leak is unmistakable.
    let missing = folder.join(format!("domovoi-{SECRET}.db"));

    let lines = Lines::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(lines.clone())
        .with_max_level(tracing::Level::TRACE)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    // The verification reports the path to its caller; it must not also log it.
    let err = verify_backup(&missing, Depth::Quick).await.unwrap_err();
    let reported = err.to_string();
    assert!(
        reported.contains(SECRET),
        "the error still carries the diagnostic detail for the caller: {reported}"
    );

    let logged = lines.text();
    assert!(
        !logged.contains(SECRET),
        "the verification path leaked the secret into a log before redaction: {logged}"
    );
    assert!(
        !logged.contains("did not verify"),
        "the low-level warning is gone, so only the redacting caller logs: {logged}"
    );
}

#[tokio::test]
async fn the_backup_verification_step_logs_nothing_before_redaction() {
    let dir = tempfile::tempdir().unwrap();
    let damaged = damaged_backup_at(dir.path(), SECRET).await;

    let lines = Lines::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(lines.clone())
        .with_max_level(tracing::Level::TRACE)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    // The exact step `backup_to` runs: verify, then remove the copy that failed.
    let err = verify_or_track_for_tests(&damaged).await.unwrap_err();
    assert!(
        err.to_string().contains(&damaged.display().to_string()),
        "the caller is told which copy failed: {err}"
    );
    assert!(!damaged.exists(), "the failed copy was removed");

    let logged = lines.text();
    assert!(
        !logged.contains(SECRET),
        "the backup verification step leaked the path into a log before redaction: {logged}"
    );
    assert!(
        !logged.contains("did not verify"),
        "no low-level warning reaches the log: {logged}"
    );
}

#[tokio::test]
async fn the_removal_helper_logs_nothing_before_redaction() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join(format!("backups-{SECRET}"));
    std::fs::create_dir_all(&folder).unwrap();
    let stuck = folder.join(format!("domovoi-{SECRET}.db"));
    std::fs::write(&stuck, b"would not verify").unwrap();

    let lines = Lines::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(lines.clone())
        .with_max_level(tracing::Level::TRACE)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let _ = verify_or_tracked_for_tests(&stuck, "damaged").await;

    let logged = lines.text();
    assert!(
        !logged.contains(SECRET),
        "the removal helper leaked the path into a log before redaction: {logged}"
    );
}

#[tokio::test]
async fn a_successful_backup_logs_nothing_from_the_verification_path() {
    let dir = tempfile::tempdir().unwrap();
    let fresh = Fresh::new().await;
    let lines = Lines::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(lines.clone())
        .with_max_level(tracing::Level::TRACE)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    let target = fresh
        .store
        .backup_to(&dir.path().join("nightly"), T0)
        .await
        .unwrap();
    fresh.store.close().await;

    assert!(target.exists());
    let logged = lines.text();
    assert!(
        !logged.contains(SECRET) && !logged.contains("did not verify"),
        "a good backup logs no verification warning: {logged}"
    );
}
