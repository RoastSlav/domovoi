//! `firewall.confirm`: step two of a block, one `delete` and `add` batch.
//!
//! The checks of `apply` run again because the safety set may have changed inside the 30
//! seconds; a refusal here lets the element expire, which fails safe. The batch is one
//! transaction, so a `delete` that finds nothing leaves nothing behind and never re-creates an
//! expired element. The batch is never retried blindly.

use domovoi_core::containment::templates::SetName;
use domovoi_core::containment::{
    AbsentClass, JournalOp, JournalRecord, NftOp, StderrClass, Token, classify_stderr, keys,
    templates,
};
use jiff::SignedDuration;
use uuid::Uuid;

use super::apply::{checks, outputs_for, seconds};
use super::classify::absent;
use super::exec::Run;
use super::flows::bridge_gap;
use super::journal_file::{Phase, fold};
use super::verify::probe;
use super::{Cx, Expected, Refusal, Reply, State};

/// Serves `firewall.confirm`.
pub async fn confirm(
    cx: &Cx<'_>,
    st: &State,
    block: Uuid,
    source: &str,
    ttl_s: u32,
    expected: Expected<'_>,
) -> Reply {
    let (net, _) = checks(cx, st, source, ttl_s, expected)?;
    let id = block.to_string();
    let now = cx.clock.now();
    let known = fold(&st.loaded.records).into_iter().find(|b| b.block == id);
    let Some(known) = known.filter(|b| !matches!(b.phase, Phase::Failed | Phase::Noop)) else {
        return Err(Refusal::new(
            Token::UnknownBlock,
            "no applied block has this id",
        ));
    };
    match known.phase {
        Phase::Lost => return Err(Refusal::new(Token::ContainmentLost, "element_missing")),
        Phase::RolledBack | Phase::RollbackIntent => {
            return Err(Refusal::new(Token::RolledBack, "the block was rolled back"));
        }
        _ => {}
    }
    if known.source != source || known.ttl_s != ttl_s {
        return Err(Refusal::new(
            Token::BlockMismatch,
            "source or ttl differ from the apply",
        ));
    }
    let script = templates::confirm(&cx.table, &net, seconds(ttl_s)?)
        .map_err(|_| Refusal::new(Token::TtlTooShort, "the TTL must be above 30s"))?;
    cx.journal
        .append(
            cx.guard,
            &JournalRecord::Intent {
                op: JournalOp::Confirm,
                block: id.clone(),
                source: source.to_owned(),
                set: SetName::for_net(&net).as_str().to_owned(),
                ttl_s,
                at: now,
            },
        )
        .map_err(Refusal::journal)?;
    let run = cx
        .run
        .script(&script)
        .await
        .map_err(|f| Refusal::of_fault(&f))?;
    if !run.ok() {
        return Err(after_failure(cx, &run, &net, &id, &known).await);
    }
    let Some(view) = super::ensure::read_table(cx).await? else {
        return Err(Refusal::new(Token::ContainmentLost, "element_missing"));
    };
    let before = st.ens.view.counters.first().copied();
    let report = probe(&view, st.ens.layout, &net, i64::from(ttl_s), before);
    if !report.passes() {
        return Err(Refusal::new(
            Token::PostconditionFailed,
            "the element has no time left or the chain is not as the template says",
        ));
    }
    let remaining = report.remaining.unwrap_or(0);
    let deadline = cx.clock.now() + SignedDuration::from_secs(remaining);
    cx.journal
        .append(
            cx.guard,
            &JournalRecord::Confirmed {
                block: id.clone(),
                deadline,
            },
        )
        .map_err(Refusal::journal)?;
    let set = SetName::for_net(&net).as_str();
    let mut out = outputs_for(cx, st, &id, source, set, "confirm");
    out.insert(keys::OUT_REMAINING_S.to_owned(), remaining.to_string());
    out.insert(
        keys::OUT_BRIDGE_GAP.to_owned(),
        bridge_gap(cx.sys_root).to_string(),
    );
    out.insert(keys::OUT_CHAIN_OK.to_owned(), report.counter_ok.to_string());
    out.insert(keys::OUT_ARRIVALS.to_owned(), report.arrivals.to_string());
    Ok(out)
}

/// What a failed batch means: expired, rolled back, covered, lost, or a token of its own.
async fn after_failure(
    cx: &Cx<'_>,
    run: &Run,
    net: &domovoi_core::containment::Net,
    id: &str,
    known: &super::journal_file::BlockState,
) -> Refusal {
    let class = classify_stderr(NftOp::Confirm, &run.stderr);
    if !matches!(
        class,
        StderrClass::ElementAbsent | StderrClass::TableOrSetMissing
    ) {
        return Refusal::from_run(NftOp::Confirm, run, true);
    }
    let now = cx.clock.now();
    let (verdict, _) = match absent(cx, net, Some(known), now).await {
        Ok(found) => found,
        Err(refusal) => return refusal,
    };
    match verdict {
        AbsentClass::TableMissing | AbsentClass::AbsentUnexplained => {
            let _ = cx.journal.append(
                cx.guard,
                &JournalRecord::Lost {
                    block: id.to_owned(),
                    at: now,
                },
            );
            Refusal::new(Token::ContainmentLost, "element_missing")
        }
        other => Refusal::new(other.token(), other_detail(other)),
    }
}

fn other_detail(class: AbsentClass) -> &'static str {
    match class {
        AbsentClass::Expired => "the kernel removed the element",
        AbsentClass::RolledBack => "the block was rolled back",
        AbsentClass::CoveredByOther => "another element covers the target",
        AbsentClass::PresentUnexpectedly => "the element is present after a failed batch",
        AbsentClass::TableMissing | AbsentClass::AbsentUnexplained => "element_missing",
    }
}
