//! `firewall.rollback`: remove a block, idempotently.
//!
//! Rollback is never refused for a safety reason. It still needs the start checks, the lock, the
//! journal and a parsable target, but it skips the identity comparison, the overlap check, the
//! limits and the mutation limiter, because it only removes. A covering element is reported and
//! never deleted.

use domovoi_core::containment::templates::SetName;
use domovoi_core::containment::{
    AbsentClass, JournalRecord, Net, NftOp, StderrClass, Token, classify_stderr, templates,
};
use uuid::Uuid;

use super::apply::outputs_for;
use super::classify::{absent, membership};
use super::journal_file::{Phase, fold};
use super::{Cx, OUT_COVERING, OUT_OUTCOME, Refusal, Reply, State};

fn parse(source: &str) -> Result<Net, Refusal> {
    use domovoi_core::containment::NetError;
    let net = Net::parse(source).map_err(|e| match e {
        NetError::MappedForm => {
            Refusal::new(Token::MappedForm, "IPv4-mapped IPv6 ranges are refused")
        }
        _ => Refusal::new(Token::InvalidTarget, e.to_string()),
    })?;
    if net.canonical() == source {
        Ok(net)
    } else {
        Err(Refusal::new(
            Token::InvalidTarget,
            "the source is not in canonical form",
        ))
    }
}

/// Serves `firewall.rollback`.
pub async fn rollback(cx: &Cx<'_>, st: &State, block: Uuid, source: &str, _ttl_s: u32) -> Reply {
    let net = parse(source)?;
    let id = block.to_string();
    if st.ens.lost.contains(&id) {
        return Err(Refusal::new(Token::ContainmentLost, "element_missing"));
    }
    let known = fold(&st.loaded.records).into_iter().find(|b| b.block == id);
    if known.as_ref().is_some_and(|b| b.phase == Phase::Lost) {
        return Err(Refusal::new(Token::ContainmentLost, "element_missing"));
    }
    let now = cx.clock.now();
    cx.journal
        .append(
            cx.guard,
            &JournalRecord::RollbackIntent {
                block: id.clone(),
                at: now,
            },
        )
        .map_err(Refusal::journal)?;
    let run = cx
        .run
        .script(&templates::rollback(&cx.table, &net))
        .await
        .map_err(|f| Refusal::of_fault(&f))?;
    let outcome = if run.ok() {
        Outcome::RolledBack
    } else {
        settle(
            cx,
            &net,
            &id,
            known.as_ref(),
            classify_stderr(NftOp::Rollback, &run.stderr),
            &run,
        )
        .await?
    };
    if !matches!(outcome, Outcome::Covered(_)) {
        cx.journal
            .append(
                cx.guard,
                &JournalRecord::RolledBack {
                    block: id.clone(),
                    at: cx.clock.now(),
                },
            )
            .map_err(Refusal::journal)?;
    }
    let set = SetName::for_net(&net).as_str();
    let mut out = outputs_for(cx, st, &id, source, set, "rollback");
    match outcome {
        Outcome::RolledBack => out.insert(OUT_OUTCOME.to_owned(), "rolled_back".to_owned()),
        Outcome::Absent => out.insert(OUT_OUTCOME.to_owned(), "absent".to_owned()),
        Outcome::Covered(by) => {
            out.insert(OUT_COVERING.to_owned(), by);
            out.insert(
                OUT_OUTCOME.to_owned(),
                Token::AbsentCovered.as_str().to_owned(),
            )
        }
    };
    Ok(out)
}

enum Outcome {
    RolledBack,
    Absent,
    Covered(String),
}

async fn fallback(cx: &Cx<'_>, net: &Net) -> Result<Outcome, Refusal> {
    let run = cx
        .run
        .script(&templates::rollback_fallback(&cx.table, net))
        .await
        .map_err(|f| Refusal::of_fault(&f))?;
    if run.ok() {
        Ok(Outcome::RolledBack)
    } else {
        Err(Refusal::from_run(NftOp::Rollback, &run, true))
    }
}

/// Decides what a failed primary batch means, using the table and the membership, never the
/// text alone.
async fn settle(
    cx: &Cx<'_>,
    net: &Net,
    id: &str,
    known: Option<&super::journal_file::BlockState>,
    class: StderrClass,
    run: &super::exec::Run,
) -> Result<Outcome, Refusal> {
    match class {
        StderrClass::SetFull | StderrClass::IntervalOverlap => {
            let found = membership(cx, net).await?;
            if !found.table_exists {
                return lost(cx, id);
            }
            if found.exact {
                return fallback(cx, net).await;
            }
            Ok(found.covering.map_or(Outcome::Absent, Outcome::Covered))
        }
        StderrClass::ElementAbsent | StderrClass::TableOrSetMissing => {
            let (verdict, found) = absent(cx, net, known, cx.clock.now()).await?;
            match verdict {
                AbsentClass::TableMissing => lost(cx, id),
                AbsentClass::PresentUnexpectedly => fallback(cx, net).await,
                AbsentClass::CoveredByOther => {
                    Ok(Outcome::Covered(found.covering.unwrap_or_default()))
                }
                _ if class == StderrClass::TableOrSetMissing => Err(Refusal::new(
                    Token::TableDrift,
                    "a set of the table is missing",
                )),
                _ => Ok(Outcome::Absent),
            }
        }
        _ => Err(Refusal::from_run(NftOp::Rollback, run, true)),
    }
}

fn lost(cx: &Cx<'_>, id: &str) -> Result<Outcome, Refusal> {
    let _ = cx.journal.append(
        cx.guard,
        &JournalRecord::Lost {
            block: id.to_owned(),
            at: cx.clock.now(),
        },
    );
    Err(Refusal::new(Token::ContainmentLost, "element_missing"))
}
