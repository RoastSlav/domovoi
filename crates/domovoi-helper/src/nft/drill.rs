//! The drill suite behind `domovoi helper containment-drill`, and the file that records it.
//!
//! The drills run the helper's own operations against a table of the production layout in a
//! throwaway network namespace, with synthetic facts (an open gate and a management network of
//! `192.0.2.0/24`) and a scratch state directory. They never touch the default namespace or the
//! production table. A result is valid for the kernel, `nft` version and layout it ran under; a
//! different key reads as `not_run`.

use std::collections::BTreeMap;
use std::path::Path;

use domovoi_core::attest::{BootId, ExpectedSafety, SafetyIdentity, UnverifiedReason};
use domovoi_core::containment::templates::{MaxElements, TableName};
use domovoi_core::containment::{
    DrillsState, ExclusionInputs, Gate, Layout, Net, SelftestKey, Token, effective_exclusions,
};
use domovoi_core::host::KeyFingerprint;
use jiff::SignedDuration;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::exec::Runner;
use super::journal_file::Journal;
use super::selftest::scenario::{Host, WorldError};
use super::{Cx, DRILLS_FILE, Facts, Refusal, State, apply, confirm, ensure, list, rollback};
use crate::state::{LockGuard, StateLock, WriteFaults, write_atomic_locked};

/// The table the drills use. Never the production name.
const TABLE: &str = "domovoi_drill";
/// The management network every drill treats as excluded.
const MANAGEMENT: &str = "192.0.2.0/24";

/// What `drills.json` holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrillsFile {
    /// What the result is valid for.
    pub key: SelftestKey,
    /// Whether every drill passed.
    pub state: DrillsState,
    /// Each drill's name and `ok` or the reason it failed.
    pub results: BTreeMap<String, String>,
}

/// The stored drill file, or `None`.
#[must_use]
pub fn read(dir: &Path) -> Option<DrillsFile> {
    serde_json::from_str(&std::fs::read_to_string(dir.join(DRILLS_FILE)).ok()?).ok()
}

/// The drill state for `key`: `not_run` when there is no file or it is for another key.
#[must_use]
pub fn state_of(dir: &Path, key: &SelftestKey) -> DrillsState {
    read(dir)
        .filter(|f| f.key == *key)
        .map_or(DrillsState::NotRun, |f| f.state)
}

/// The drill state as JSON text for `list_blocks`.
#[must_use]
pub fn state_text(dir: &Path, key: &SelftestKey) -> String {
    serde_json::to_string(&state_of(dir, key))
        .unwrap_or_else(|_| "{\"state\":\"not_run\"}".to_owned())
}

/// Writes the drill file atomically under the held lock.
pub fn write(guard: &LockGuard<'_>, dir: &Path, file: &DrillsFile) -> Result<(), String> {
    let text = serde_json::to_vec_pretty(file).map_err(|e| e.to_string())?;
    write_atomic_locked(
        guard,
        dir,
        DRILLS_FILE,
        &text,
        0o600,
        WriteFaults::default(),
    )
    .map_err(|e| e.to_string())
}

fn fail(why: &str) -> String {
    why.to_owned()
}

fn synthetic_identity() -> Option<SafetyIdentity> {
    let zero = domovoi_core::attest::ZERO_HASH;
    Some(SafetyIdentity {
        fingerprint: KeyFingerprint::of_public_key(&[7; 32]),
        boot_id: BootId::parse("00000000-0000-4000-8000-000000000000").ok()?,
        generation: 1,
        config_hash: zero,
        route_state_hash: zero,
    })
}

fn facts() -> Result<Facts, String> {
    let management = Net::parse(MANAGEMENT).map_err(|e| e.to_string())?;
    let inputs = ExclusionInputs {
        management_cidrs: vec![management],
        ..ExclusionInputs::default()
    };
    let exclusions = effective_exclusions(Some(&inputs)).map_err(|e| e.to_string())?;
    Ok(Facts {
        gate: Gate::Open {
            interval_path: true,
        },
        exclusions: Ok(exclusions),
        identity: synthetic_identity().ok_or(()),
        key_text: String::new(),
        drills: String::new(),
    })
}

struct Env<'a> {
    host: &'a Host<'a>,
    rx: String,
    dir: &'a Path,
}

impl Env<'_> {
    fn cx<'c>(&'c self, guard: &'c LockGuard<'c>, max_active: u32) -> Result<Cx<'c>, String> {
        Ok(Cx {
            run: Runner {
                exec: self.host.exec,
                inspect: self.host.inspect,
                timeout: SignedDuration::from_secs(5),
                netns: Some(self.rx.as_str()),
            },
            clock: self.host.clock,
            table: TableName::parse(TABLE).map_err(|e| e.to_string())?,
            max_elements: MaxElements::new(1000).map_err(|e| e.to_string())?,
            max_ttl_s: 3600,
            max_active,
            mutations_per_hour: 1000,
            dir: self.dir,
            sys_root: Path::new("/sys"),
            journal: Journal::new(self.dir),
            guard,
        })
    }

    async fn state(&self, cx: &Cx<'_>) -> Result<State, String> {
        let loaded = cx.journal.load().map_err(|e| e.to_string())?;
        let facts = facts()?;
        let exclusions = facts.exclusions.clone().map_err(|r| r.message())?;
        let ens = ensure::ensure(
            cx,
            Some(Layout::HostsAndPrefixes),
            Some(&exclusions),
            &loaded,
        )
        .await
        .map_err(|r| r.message())?;
        Ok(State { ens, loaded, facts })
    }
}

fn unverified() -> ExpectedSafety {
    ExpectedSafety::Unverified {
        reason: UnverifiedReason::KeyNotPinned,
    }
}

fn token_is(result: &Result<super::Outputs, Refusal>, token: Token) -> bool {
    matches!(result, Err(r) if r.token == token)
}

async fn apply_one(
    env: &Env<'_>,
    cx: &Cx<'_>,
    block: Uuid,
    source: &str,
    ttl: u32,
) -> Result<(), String> {
    let st = env.state(cx).await?;
    apply::apply(cx, &st, block, source, ttl, Some(&unverified()))
        .await
        .map(|_| ())
        .map_err(|r| r.message())
}

async fn release(env: &Env<'_>, cx: &Cx<'_>, block: Uuid, source: &str, ttl: u32) {
    if let Ok(st) = env.state(cx).await {
        let _ = rollback::rollback(cx, &st, block, source, ttl).await;
    }
}

async fn excluded_address_refused(env: &Env<'_>, cx: &Cx<'_>) -> Result<(), String> {
    let st = env.state(cx).await?;
    let r = apply::apply(
        cx,
        &st,
        Uuid::now_v7(),
        "192.0.2.7/32",
        600,
        Some(&unverified()),
    )
    .await;
    token_is(&r, Token::ExclusionOverlap)
        .then_some(())
        .ok_or_else(|| fail("an address inside the management network was not refused"))
}

async fn max_active_refused(env: &Env<'_>, guard: &LockGuard<'_>) -> Result<(), String> {
    let cx = env.cx(guard, 1)?;
    let first = Uuid::now_v7();
    apply_one(env, &cx, first, "203.0.113.10/32", 600).await?;
    let st = env.state(&cx).await?;
    let second = apply::apply(
        &cx,
        &st,
        Uuid::now_v7(),
        "203.0.113.11/32",
        600,
        Some(&unverified()),
    )
    .await;
    release(env, &cx, first, "203.0.113.10/32", 600).await;
    token_is(&second, Token::MaxActiveReached)
        .then_some(())
        .ok_or_else(|| fail("a second block was accepted at max_active 1"))
}

async fn duplicate_is_idempotent(env: &Env<'_>, cx: &Cx<'_>) -> Result<(), String> {
    let block = Uuid::now_v7();
    apply_one(env, cx, block, "203.0.113.20/32", 600).await?;
    let again = apply_one(env, cx, block, "203.0.113.20/32", 600).await;
    let other = {
        let st = env.state(cx).await?;
        apply::apply(
            cx,
            &st,
            Uuid::now_v7(),
            "203.0.113.20/32",
            600,
            Some(&unverified()),
        )
        .await
    };
    release(env, cx, block, "203.0.113.20/32", 600).await;
    again?;
    token_is(&other, Token::AlreadyPresent)
        .then_some(())
        .ok_or_else(|| fail("an element that was already there was not reported as present"))
}

/// An unconfirmed block must be gone within its 30 s kernel timeout, with nobody left to remove it.
async fn expiry_without_confirm(env: &Env<'_>, cx: &Cx<'_>) -> Result<(), String> {
    apply_one(env, cx, Uuid::now_v7(), "203.0.113.30/32", 600).await?;
    env.host
        .clock
        .sleep_until(env.host.clock.now() + SignedDuration::from_secs(33))
        .await;
    let st = env.state(cx).await?;
    let gone = st
        .ens
        .view
        .sets
        .get("blocked4")
        .is_some_and(|s| s.members.is_empty());
    gone.then_some(())
        .ok_or_else(|| fail("the element was still in the set after 33 seconds"))
}

/// A helper process that starts again mid-TTL finds the block through its files and the table.
async fn restart_mid_ttl(env: &Env<'_>, guard: &LockGuard<'_>) -> Result<(), String> {
    let block = Uuid::now_v7();
    let source = "203.0.113.40/32";
    {
        let cx = env.cx(guard, 10)?;
        apply_one(env, &cx, block, source, 45).await?;
        let st = env.state(&cx).await?;
        confirm::confirm(&cx, &st, block, source, 45, Some(&unverified()))
            .await
            .map_err(|r| r.message())?;
    }
    let cx = env.cx(guard, 10)?;
    let st = env.state(&cx).await?;
    let listed = list::list(&cx, &st).await.map_err(|r| r.message())?;
    release(env, &cx, block, source, 45).await;
    let blocks = listed.get("blocks").cloned().unwrap_or_default();
    blocks
        .contains(source)
        .then_some(())
        .ok_or_else(|| fail("the block was not found after the helper started again"))
}

/// Runs every drill and returns each one's result.
pub async fn run(host: &Host<'_>, scratch: &Path) -> Result<BTreeMap<String, String>, WorldError> {
    let env = Env {
        host,
        rx: format!("domovoi-st-{}-rx", host.id),
        dir: scratch,
    };
    let lock = StateLock::open(scratch).map_err(|e| WorldError(e.to_string()))?;
    let guard = lock.acquire().map_err(|e| WorldError(e.to_string()))?;
    let cx = env.cx(&guard, 10).map_err(WorldError)?;
    let mut results = BTreeMap::new();
    let mut record = |name: &str, r: Result<(), String>| {
        results.insert(name.to_owned(), r.err().unwrap_or_else(|| "ok".to_owned()));
    };
    record(
        "drill_excluded_address_refused",
        excluded_address_refused(&env, &cx).await,
    );
    record(
        "drill_max_active_reached_is_refused",
        max_active_refused(&env, &guard).await,
    );
    record(
        "drill_duplicate_dispatch_is_idempotent",
        duplicate_is_idempotent(&env, &cx).await,
    );
    record(
        "drill_agent_restart_mid_ttl",
        restart_mid_ttl(&env, &guard).await,
    );
    record(
        "drill_expiry_on_time_with_agent_stopped",
        expiry_without_confirm(&env, &cx).await,
    );
    let unconfirmed = results
        .get("drill_expiry_on_time_with_agent_stopped")
        .cloned()
        .unwrap_or_default();
    results.insert(
        "drill_hub_unreachable_during_confirmation_window_block_gone_within_30s".to_owned(),
        unconfirmed,
    );
    Ok(results)
}
