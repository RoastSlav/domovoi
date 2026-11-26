//! The leak self-test: does replacing an element ever let a packet through?
//!
//! It runs as the separate subcommand `domovoi helper nft-selftest`, from the installer and from
//! a one-shot unit ordered before the helper socket, never inside the long-lived helper. A
//! result is valid for one kernel release, one `nft` version and one layout. A failure to run is
//! never a pass; a minimum not reached is inconclusive and blocks.

pub mod scenario;
pub mod sender;
pub mod verdict;

use std::path::Path;

use domovoi_core::containment::{Gate, Layout, SelftestKey, SelftestResult, Verdict};
use jiff::{SignedDuration, Timestamp};

use self::scenario::{Host, run_scenario, scenarios};
use self::verdict::{Counts, lowest_control_ratio, lowest_replacements, verdict};
use super::SELFTEST_FILE;
use crate::state::{LockGuard, WriteFaults, write_atomic_locked};

/// How long an inconclusive result is trusted before `--if-changed` runs the test again.
const INCONCLUSIVE_MAX_AGE: SignedDuration = SignedDuration::from_hours(7 * 24);

/// The key a result is valid for. The layout is always the one that holds both set types.
#[must_use]
pub fn current_key(kernel_release: &str, nft_version: &str) -> SelftestKey {
    SelftestKey {
        kernel_release: kernel_release.to_owned(),
        nft_version: nft_version.to_owned(),
        layout: Layout::HostsAndPrefixes,
    }
}

/// The stored result, or `None` when there is none or it cannot be read.
#[must_use]
pub fn read_result(dir: &Path) -> Option<SelftestResult> {
    let text = std::fs::read_to_string(dir.join(SELFTEST_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Writes the result atomically under the held lock.
pub fn write_result(
    guard: &LockGuard<'_>,
    dir: &Path,
    result: &SelftestResult,
) -> Result<(), String> {
    let text = serde_json::to_vec_pretty(result).map_err(|e| e.to_string())?;
    write_atomic_locked(
        guard,
        dir,
        SELFTEST_FILE,
        &text,
        0o600,
        WriteFaults::default(),
    )
    .map_err(|e| e.to_string())
}

/// The gate as text for `list_blocks`: the state, the reason and the key it was computed for.
#[must_use]
pub fn gate_text(gate: &Gate, key: &SelftestKey) -> String {
    serde_json::json!({
        "gate": gate,
        "kernel": key.kernel_release,
        "nft": key.nft_version,
        "layout": key.layout.as_str(),
    })
    .to_string()
}

/// Whether `--if-changed` may skip the run: the stored result has the current key and is not an
/// inconclusive one older than seven days.
#[must_use]
pub fn up_to_date(stored: Option<&SelftestResult>, key: &SelftestKey, now: Timestamp) -> bool {
    let Some(result) = stored else {
        return false;
    };
    let inconclusive = matches!(result.hosts, Verdict::Inconclusive { .. })
        || matches!(result.prefixes, Some(Verdict::Inconclusive { .. }));
    result.key == *key
        && !(inconclusive && now.duration_since(result.ran_at) > INCONCLUSIVE_MAX_AGE)
}

/// What a run of the self-test produced.
pub struct Run {
    /// The result to store.
    pub result: SelftestResult,
    /// What each scenario measured.
    pub counts: Vec<Counts>,
}

/// Runs the scenarios and builds the result. The namespaces are deleted before this returns,
/// whether the scenarios finished or not.
///
/// `only_prefixes` re-tests the interval sets and keeps the stored plain-set verdict.
pub async fn run(
    host: &Host<'_>,
    key: SelftestKey,
    stored: Option<&SelftestResult>,
    only_prefixes: bool,
) -> Result<Run, scenario::WorldError> {
    let outcome = measure(host, only_prefixes).await;
    host.teardown().await;
    let (hosts, prefixes) = outcome?;
    let kept = stored.filter(|r| r.key == key).map(|r| r.hosts.clone());
    let hosts_verdict = match (only_prefixes, hosts.is_empty(), kept) {
        (true, true, Some(kept)) => kept,
        (_, _, _) => verdict(&hosts),
    };
    let mut all: Vec<Counts> = hosts.clone();
    all.extend(prefixes.clone());
    let result = SelftestResult {
        key,
        hosts: hosts_verdict,
        prefixes: Some(verdict(&prefixes)),
        ran_at: host.clock.now(),
        replacements: lowest_replacements(&all),
        control_leak_ratio: lowest_control_ratio(&all),
    };
    Ok(Run {
        result,
        counts: all,
    })
}

async fn measure(
    host: &Host<'_>,
    only_prefixes: bool,
) -> Result<(Vec<Counts>, Vec<Counts>), scenario::WorldError> {
    host.build().await?;
    let mut hosts = Vec::new();
    if !only_prefixes {
        for s in scenarios(false) {
            hosts.push(run_scenario(host, s).await?);
        }
    }
    let mut prefixes = Vec::new();
    for s in scenarios(true) {
        prefixes.push(run_scenario(host, s).await?);
    }
    Ok((hosts, prefixes))
}
