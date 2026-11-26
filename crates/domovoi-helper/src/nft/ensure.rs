//! `ensure_table`: create the table when it is absent, compare it with the template when it is
//! not, and keep the exclusion sets equal to the computed set.
//!
//! The bootstrap starts with `create table`, so it can never run over a live table. A live table
//! is read as JSON and compared by name and number; a difference is `table_drift` and nothing
//! repairs it, except that an empty table of the wrong layout is deleted and made again.

use domovoi_core::containment::templates::{CHAIN_PRIORITY, STEP1_TTL_S, SetName};
use domovoi_core::containment::{
    Family, JournalRecord, Layout, Net, NftOp, RecreateReason, StderrClass, Token, classify_stderr,
    coalesce, templates,
};

use super::journal_file::{Loaded, fold};
use super::view::{TableView, is_absent, parse};
use super::{Cx, Refusal, TableState};

/// What `ensure_table` found and did.
#[derive(Debug, Clone)]
pub struct Ensured {
    /// The table as it is now.
    pub view: TableView,
    /// Whether it existed, was created or was created again.
    pub state: TableState,
    /// The first difference from the template; new containment is refused while it is set.
    pub drift: Option<String>,
    /// The blocks this request found lost because the table was gone.
    pub lost: Vec<String>,
    /// The layout of the table.
    pub layout: Layout,
}

struct SetSpec {
    name: &'static str,
    typ: &'static str,
    flags: &'static [&'static str],
    timed: bool,
}

fn set_specs(layout: Layout) -> Vec<SetSpec> {
    let mut specs = vec![
        SetSpec {
            name: "blocked4",
            typ: "ipv4_addr",
            flags: &["timeout"],
            timed: true,
        },
        SetSpec {
            name: "blocked6",
            typ: "ipv6_addr",
            flags: &["timeout"],
            timed: true,
        },
        SetSpec {
            name: "excl4",
            typ: "ipv4_addr",
            flags: &["interval"],
            timed: false,
        },
        SetSpec {
            name: "excl6",
            typ: "ipv6_addr",
            flags: &["interval"],
            timed: false,
        },
    ];
    if layout == Layout::HostsAndPrefixes {
        specs.push(SetSpec {
            name: "blockednet4",
            typ: "ipv4_addr",
            flags: &["interval", "timeout"],
            timed: true,
        });
        specs.push(SetSpec {
            name: "blockednet6",
            typ: "ipv6_addr",
            flags: &["interval", "timeout"],
            timed: true,
        });
    }
    specs
}

fn rule_specs(layout: Layout) -> Vec<&'static str> {
    let mut rules = vec![
        "counter",
        "ip.saddr@excl4:accept",
        "ip6.saddr@excl6:accept",
        "ip.saddr@blocked4:counter,drop",
        "ip6.saddr@blocked6:counter,drop",
    ];
    if layout == Layout::HostsAndPrefixes {
        rules.push("ip.saddr@blockednet4:counter,drop");
        rules.push("ip6.saddr@blockednet6:counter,drop");
    }
    rules
}

/// The layout the listing shows: the interval sets are there or they are not.
#[must_use]
pub fn layout_of(view: &TableView) -> Layout {
    if view.sets.contains_key("blockednet4") || view.sets.contains_key("blockednet6") {
        Layout::HostsAndPrefixes
    } else {
        Layout::Hosts
    }
}

/// The first way `view` differs from the template of `layout`, or `None` when they match.
#[must_use]
pub fn drift_of(view: &TableView, layout: Layout, max_elements: u32) -> Option<String> {
    for spec in set_specs(layout) {
        let Some(set) = view.sets.get(spec.name) else {
            return Some(format!("set {} is missing", spec.name));
        };
        let flags: std::collections::BTreeSet<String> =
            spec.flags.iter().map(|f| (*f).to_owned()).collect();
        if set.typ != spec.typ || set.flags != flags {
            return Some(format!("set {} has another type or flags", spec.name));
        }
        if spec.timed
            && (set.size != Some(u64::from(max_elements))
                || set.timeout != Some(u64::from(STEP1_TTL_S)))
        {
            return Some(format!(
                "set {} has another size or default timeout",
                spec.name
            ));
        }
    }
    let extra = view
        .sets
        .keys()
        .any(|k| !set_specs(layout).iter().any(|s| s.name == k));
    if extra {
        return Some("the table holds a set the template does not".to_owned());
    }
    chain_drift(view, layout)
}

/// The first way the chain or its rules differ from the template, or `None`.
#[must_use]
pub fn chain_drift(view: &TableView, layout: Layout) -> Option<String> {
    match &view.chain {
        Some(chain)
            if chain.typ == "filter"
                && chain.hook == "prerouting"
                && chain.prio == i64::from(CHAIN_PRIORITY)
                && chain.policy == "accept" => {}
        _ => {
            return Some(
                "the contain chain is missing or has another hook, priority or policy".to_owned(),
            );
        }
    }
    if view.rules != rule_specs(layout) {
        return Some("the rules of the contain chain differ from the template".to_owned());
    }
    None
}

fn token_of(class: StderrClass, table_exists: bool) -> Token {
    class
        .token(false, table_exists)
        .unwrap_or(Token::InternalTemplateError)
}

/// Reads the table: `Ok(None)` when it does not exist.
pub async fn read_table(cx: &Cx<'_>) -> Result<Option<TableView>, Refusal> {
    let run = cx
        .run
        .read(templates::list_table_args(&cx.table))
        .await
        .map_err(|f| Refusal::of_fault(&f))?;
    if run.ok() {
        return parse(&run.stdout).map(Some).map_err(|_| {
            Refusal::new(
                Token::InternalTemplateError,
                "the table listing is not JSON",
            )
        });
    }
    if is_absent(run.exit, &run.stderr) {
        return Ok(None);
    }
    Err(Refusal::from_run(NftOp::Read, &run, true))
}

async fn bootstrap(cx: &Cx<'_>, layout: Layout) -> Result<(), Refusal> {
    let script = templates::bootstrap(&cx.table, layout, cx.max_elements);
    let run = cx
        .run
        .script(&script)
        .await
        .map_err(|f| Refusal::of_fault(&f))?;
    if run.ok() {
        return Ok(());
    }
    match classify_stderr(NftOp::CreateTable, &run.stderr) {
        StderrClass::TableExistsRace => Ok(()),
        class => Err(Refusal::new(
            token_of(class, false),
            first_line(&run.stderr),
        )),
    }
}

fn first_line(text: &str) -> String {
    text.lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(160)
        .collect()
}

fn exclusion_ranges(net_list: &[Net], family: Family) -> Vec<(u128, u128)> {
    let of_family: Vec<Net> = net_list
        .iter()
        .filter(|n| n.family() == family)
        .copied()
        .collect();
    coalesce(&of_family)
        .into_iter()
        .map(|(lo, hi, _)| (lo, hi))
        .collect()
}

fn current_ranges(view: &TableView, set: SetName) -> Vec<(u128, u128)> {
    let mut ranges: Vec<(u128, u128)> = view
        .sets
        .get(set.as_str())
        .map(|s| s.members.iter().map(|m| (m.lo, m.hi)).collect())
        .unwrap_or_default();
    ranges.sort_unstable();
    ranges
}

/// Rewrites the exclusion sets that differ from `exclusions`. Returns whether anything was sent.
async fn sync_exclusions(
    cx: &Cx<'_>,
    view: &TableView,
    exclusions: &[Net],
) -> Result<bool, Refusal> {
    let mut sent = false;
    for family in [Family::V4, Family::V6] {
        let want = exclusion_ranges(exclusions, family);
        if current_ranges(view, SetName::exclusion(family)) == want {
            continue;
        }
        let script = templates::exclusion_rewrite(&cx.table, family, &want);
        let run = cx
            .run
            .script(&script)
            .await
            .map_err(|f| Refusal::of_fault(&f))?;
        if !run.ok() {
            return Err(Refusal::from_run(NftOp::ExclusionRewrite, &run, true));
        }
        sent = true;
    }
    Ok(sent)
}

/// Makes the table exist and match; see the module documentation.
///
/// `wanted` is the layout the self-test allows, `None` to accept either. `exclusions` is the
/// computed set, `None` to leave the exclusion sets alone (rollback and list).
pub async fn ensure(
    cx: &Cx<'_>,
    wanted: Option<Layout>,
    exclusions: Option<&[Net]>,
    loaded: &Loaded,
) -> Result<Ensured, Refusal> {
    let now = cx.clock.now();
    let live: Vec<String> = fold(&loaded.records)
        .into_iter()
        .filter(|b| b.live(now))
        .map(|b| b.block)
        .collect();
    let mut state = TableState::Ok;
    let mut lost = Vec::new();
    let mut view = match read_table(cx).await? {
        Some(view) => view,
        None => {
            let layout = wanted.unwrap_or(Layout::Hosts);
            for block in &live {
                cx.journal
                    .append(
                        cx.guard,
                        &JournalRecord::Lost {
                            block: block.clone(),
                            at: now,
                        },
                    )
                    .map_err(Refusal::journal)?;
            }
            if !live.is_empty() {
                cx.journal
                    .append(
                        cx.guard,
                        &JournalRecord::TableRecreated {
                            reason: RecreateReason::TableLost,
                            at: now,
                        },
                    )
                    .map_err(Refusal::journal)?;
            }
            lost = live.clone();
            bootstrap(cx, layout).await?;
            state = if lost.is_empty() {
                TableState::Created
            } else {
                TableState::Recreated
            };
            read_table(cx).await?.ok_or_else(|| {
                Refusal::new(Token::TableDrift, "the table is gone after its creation")
            })?
        }
    };
    let mut layout = layout_of(&view);
    if let Some(w) = wanted
        && w != layout
    {
        let empty = view
            .sets
            .iter()
            .filter(|(name, _)| name.starts_with("blocked"))
            .all(|(_, set)| set.members.is_empty());
        if empty && live.is_empty() {
            let run = cx
                .run
                .script(&templates::delete_own_table(&cx.table))
                .await
                .map_err(|f| Refusal::of_fault(&f))?;
            if !run.ok() {
                return Err(Refusal::from_run(NftOp::Bootstrap, &run, true));
            }
            bootstrap(cx, w).await?;
            cx.journal
                .append(
                    cx.guard,
                    &JournalRecord::TableRecreated {
                        reason: RecreateReason::LayoutChanged,
                        at: now,
                    },
                )
                .map_err(Refusal::journal)?;
            state = TableState::Recreated;
            view = read_table(cx).await?.ok_or_else(|| {
                Refusal::new(Token::TableDrift, "the table is gone after its creation")
            })?;
            layout = layout_of(&view);
        }
    }
    let mut drift = drift_of(&view, layout, cx.max_elements.get());
    if let Some(w) = wanted
        && w != layout
        && drift.is_none()
    {
        drift = Some("the layout differs from the one the self-test allows".to_owned());
    }
    if drift.is_none()
        && let Some(exclusions) = exclusions
        && sync_exclusions(cx, &view, exclusions).await?
    {
        view = read_table(cx).await?.ok_or_else(|| {
            Refusal::new(Token::TableDrift, "the table is gone after its rewrite")
        })?;
    }
    Ok(Ensured {
        view,
        state,
        drift,
        lost,
        layout,
    })
}
