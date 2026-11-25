//! `firewall.apply`: step one of a block, and the checks `confirm` repeats.
//!
//! Order: the table must not have drifted; the gate must be open; the exclusion set must be
//! computable; `expected_safety` must be present and, when verified, equal to the helper's own
//! identity; the target must parse, be supported on this platform, clear the prefix floor and
//! overlap no exclusion; the TTL must be in range; `max_active` and the mutation limiter must
//! allow one more. Then the journal gets its intent, the `create` runs and the probe checks it.

use std::collections::BTreeMap;

use domovoi_core::attest::{ExpectedSafety, IdentityMatch, compare};
use domovoi_core::containment::templates::{Seconds, SetName};
use domovoi_core::containment::{
    Gate, JournalOp, JournalRecord, Net, NetError, NftOp, StderrClass, TargetRefusal, Token,
    check_target, classify_stderr, keys, templates,
};
use uuid::Uuid;

use super::classify::membership;
use super::ensure::read_table;
use super::exec::Run;
use super::flows::{bridge_gap, existing_flows};
use super::journal_file::{Phase, fold};
use super::limits::{check_active, check_ttl, policy};
use super::verify::probe;
use super::{Cx, Expected, Outputs, Refusal, Reply, State};
use crate::systemd::Budget;

/// The id the mutation limiter keeps containment applies under.
const LIMITER_ID: &str = "firewall.apply";

/// The gate must be open; the refusal names the state.
pub(super) fn require_open(st: &State) -> Result<bool, Refusal> {
    match st.facts.gate {
        Gate::Open { interval_path } => Ok(interval_path),
        Gate::BlockedLeak => Err(Refusal::new(Token::ContainmentBlocked, "blocked_leak")),
        Gate::BlockedUntested { reason } => Err(Refusal::new(
            Token::ContainmentBlocked,
            format!("blocked_untested: {reason:?}"),
        )),
    }
}

fn parse_net(source: &str) -> Result<Net, Refusal> {
    let net = Net::parse(source).map_err(|e| match e {
        NetError::MappedForm => {
            Refusal::new(Token::MappedForm, "IPv4-mapped IPv6 ranges are refused")
        }
        NetError::Invalid | NetError::HostBits | NetError::PrefixLength => {
            Refusal::new(Token::InvalidTarget, e.to_string())
        }
    })?;
    if net.canonical() != source {
        return Err(Refusal::new(
            Token::InvalidTarget,
            "the source is not in canonical form",
        ));
    }
    Ok(net)
}

/// The checks `apply` and `confirm` share. Returns the parsed target and whether the interval
/// path is on.
pub(super) fn checks(
    cx: &Cx<'_>,
    st: &State,
    source: &str,
    ttl_s: u32,
    expected: Expected<'_>,
) -> Result<(Net, bool), Refusal> {
    if let Some(drift) = &st.ens.drift {
        return Err(Refusal::new(Token::TableDrift, drift.clone()));
    }
    let interval_path = require_open(st)?;
    let exclusions = st.facts.exclusions.clone()?;
    match expected {
        None => {
            return Err(Refusal::new(
                Token::MalformedSafety,
                "expected_safety is required",
            ));
        }
        Some(ExpectedSafety::Unverified { .. }) => {}
        Some(verified @ ExpectedSafety::Verified { .. }) => {
            let current = st.facts.identity.as_ref().map_err(|()| {
                Refusal::new(Token::SafetyUnknown, "the helper cannot name its identity")
            })?;
            let wanted = verified.identity().ok_or_else(|| {
                Refusal::new(Token::MalformedSafety, "expected_safety is malformed")
            })?;
            if compare(&wanted, current) != IdentityMatch::Exact {
                return Err(Refusal::new(
                    Token::SafetyChanged,
                    "the safety identity differs",
                ));
            }
        }
    }
    let net = parse_net(source)?;
    if !net.is_single_host() && !interval_path {
        return Err(Refusal::new(
            Token::PrefixUnsupportedPlatform,
            "whole networks are not enabled on this host",
        ));
    }
    check_target(&net, &exclusions, &policy()).map_err(|e| match e {
        TargetRefusal::PrefixTooWide => {
            Refusal::new(Token::PrefixTooWide, "the prefix is too wide")
        }
        TargetRefusal::ExclusionOverlap(ex) => {
            Refusal::new(Token::ExclusionOverlap, format!("the target overlaps {ex}"))
        }
    })?;
    check_ttl(ttl_s, cx.max_ttl_s)?;
    Ok((net, interval_path))
}

fn spent(cx: &Cx<'_>) -> Result<(), Refusal> {
    let refuse = || Refusal::new(Token::MutationLimit, "the mutation limit is reached");
    let now = cx.clock.now();
    let mut budget = Budget::load(cx.dir).map_err(|_| refuse())?;
    budget
        .check(LIMITER_ID, now, cx.mutations_per_hour, 0, 0)
        .map_err(|_| refuse())?;
    budget.record(LIMITER_ID, now, 0);
    budget.store(cx.guard, cx.dir).map_err(|_| refuse())
}

fn failed(cx: &Cx<'_>, block: &str, refusal: Refusal) -> Refusal {
    let _ = cx.journal.append(
        cx.guard,
        &JournalRecord::Failed {
            block: block.to_owned(),
            token: refusal.token.as_str().to_owned(),
        },
    );
    refusal
}

/// Rolls a block back after a failed probe and says so.
async fn undo(cx: &Cx<'_>, net: &Net, block: &str) {
    let now = cx.clock.now();
    let _ = cx.journal.append(
        cx.guard,
        &JournalRecord::RollbackIntent {
            block: block.to_owned(),
            at: now,
        },
    );
    if let Ok(run) = cx.run.script(&templates::rollback(&cx.table, net)).await
        && run.ok()
    {
        let _ = cx.journal.append(
            cx.guard,
            &JournalRecord::RolledBack {
                block: block.to_owned(),
                at: cx.clock.now(),
            },
        );
    }
}

fn step_failure(run: &Run, table_exists: bool) -> Refusal {
    let class = classify_stderr(NftOp::CreateElement, &run.stderr);
    let token = class.token(false, table_exists).unwrap_or(match class {
        StderrClass::TableOrSetMissing => Token::TableDrift,
        _ => Token::InternalTemplateError,
    });
    Refusal::new(token, run.stderr.lines().next().unwrap_or_default())
}

/// Serves `firewall.apply`.
pub async fn apply(
    cx: &Cx<'_>,
    st: &State,
    block: Uuid,
    source: &str,
    ttl_s: u32,
    expected: Expected<'_>,
) -> Reply {
    let (net, interval_path) = checks(cx, st, source, ttl_s, expected)?;
    let id = block.to_string();
    let set = SetName::for_net(&net).as_str();
    if let Some(known) = fold(&st.loaded.records).into_iter().find(|b| b.block == id)
        && matches!(known.phase, Phase::Applied | Phase::Confirmed)
        && known.source == source
        && known.live(cx.clock.now())
    {
        return Ok(outputs_for(cx, st, &id, source, set, "apply"));
    }
    check_active(&st.ens.view, interval_path, cx.max_active)?;
    spent(cx)?;
    let now = cx.clock.now();
    cx.journal
        .append(
            cx.guard,
            &JournalRecord::Intent {
                op: JournalOp::Apply,
                block: id.clone(),
                source: source.to_owned(),
                set: set.to_owned(),
                ttl_s,
                at: now,
            },
        )
        .map_err(Refusal::journal)?;
    let run = cx
        .run
        .script(&templates::apply(&cx.table, &net))
        .await
        .map_err(|f| failed(cx, &id, Refusal::of_fault(&f)))?;
    if !run.ok() {
        let exists = membership(cx, &net).await.is_ok_and(|m| m.table_exists);
        let refusal = step_failure(&run, exists);
        if refusal.token == Token::AlreadyPresent {
            let _ = cx
                .journal
                .append(cx.guard, &JournalRecord::Noop { block: id.clone() });
            return Err(refusal);
        }
        return Err(failed(cx, &id, refusal));
    }
    let after = read_table(cx).await?.ok_or_else(|| {
        failed(
            cx,
            &id,
            Refusal::new(Token::ContainmentLost, "the table is gone"),
        )
    })?;
    let before = st.ens.view.counters.first().copied();
    let report = probe(
        &after,
        st.ens.layout,
        &net,
        i64::from(templates::STEP1_TTL_S),
        before,
    );
    if !report.passes() {
        undo(cx, &net, &id).await;
        return Err(failed(
            cx,
            &id,
            Refusal::new(
                Token::PostconditionFailed,
                "the element or the chain is not as the template says",
            ),
        ));
    }
    cx.journal
        .append(
            cx.guard,
            &JournalRecord::Applied {
                block: id.clone(),
                at: cx.clock.now(),
            },
        )
        .map_err(Refusal::journal)?;
    let flows = existing_flows(cx, &net).await;
    let mut out = outputs_for(cx, st, &id, source, set, "apply");
    out.insert(keys::OUT_APPLIED_AT.to_owned(), cx.clock.now().to_string());
    out.insert(
        keys::OUT_STEP1_TTL_S.to_owned(),
        templates::STEP1_TTL_S.to_string(),
    );
    out.insert(
        keys::OUT_EXISTING_FLOWS.to_owned(),
        flows.as_str().to_owned(),
    );
    out.insert(keys::OUT_CHAIN_OK.to_owned(), report.counter_ok.to_string());
    out.insert(keys::OUT_ARRIVALS.to_owned(), report.arrivals.to_string());
    Ok(out)
}

/// The outputs every phase carries.
pub(super) fn outputs_for(
    cx: &Cx<'_>,
    st: &State,
    block: &str,
    source: &str,
    set: &str,
    phase: &str,
) -> Outputs {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    out.insert(keys::OUT_PHASE.to_owned(), phase.to_owned());
    out.insert(keys::OUT_BLOCK.to_owned(), block.to_owned());
    out.insert(keys::OUT_SOURCE.to_owned(), source.to_owned());
    out.insert(keys::OUT_SET.to_owned(), set.to_owned());
    out.insert(
        keys::OUT_TABLE_STATE.to_owned(),
        st.ens.state.as_str().to_owned(),
    );
    out.insert(
        keys::OUT_LAYOUT.to_owned(),
        st.ens.layout.as_str().to_owned(),
    );
    if let Ok(identity) = &st.facts.identity {
        out.insert(
            keys::OUT_GENERATION.to_owned(),
            identity.generation.to_string(),
        );
    }
    out.insert(
        keys::OUT_BRIDGE_GAP.to_owned(),
        bridge_gap(cx.sys_root).to_string(),
    );
    if !st.ens.lost.is_empty() {
        out.insert(keys::OUT_LOST_BLOCKS.to_owned(), st.ens.lost.join(","));
    }
    out
}

/// The TTL as the template type, refused when not above 30 s.
pub(super) fn seconds(ttl_s: u32) -> Result<Seconds, Refusal> {
    Seconds::new(ttl_s).map_err(|_| Refusal::new(Token::TtlTooShort, "the TTL must be at least 1s"))
}
