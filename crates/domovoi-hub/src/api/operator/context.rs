//! What the model is told about the world for one question: the hosts, the active incidents,
//! the chosen host's latest facts and, when attached, a read-only summary of an operation.

use std::collections::BTreeMap;

use domovoi_core::conversation::{Conversation, ConversationScope};
use domovoi_core::ports::{IncidentFilter, Page};
use domovoi_core::{DigestFact, FactValue, Host, Incident};
use domovoi_engine::conversations::ChatContext;
use jiff::SignedDuration;

use crate::api::ApiState;
use crate::api::dto::IncidentSummary;
use crate::api::error::ApiError;
use crate::api::hosts::{FACT_WINDOW, host_dto};

/// The most active incidents the overview lists.
const OVERVIEW_INCIDENTS: u16 = 20;

/// The longest operation goal the overview carries, in characters.
const GOAL_CHARS: usize = 300;

/// The context for a question in `conversation`.
///
/// A host that is offline, or gone, gets no tools: the model answers from the overview, the
/// same as for "all hosts". The scope itself never changes.
pub(crate) async fn chat_context(
    state: &ApiState,
    conversation: &Conversation,
) -> Result<ChatContext, ApiError> {
    let host = match conversation.scope {
        ConversationScope::Host(id) => state.deps.hosts.get_host(id).await?,
        ConversationScope::Global => None,
    };
    // An offline host has no agent to ask, so the model gets the hub's overview and no
    // tools, the same as "All hosts", rather than tools whose calls would all fail.
    let tool_host = match &host {
        Some(host) if host_dto(state, host).await?.connected => Some(host.id),
        _ => None,
    };
    let mut overview = overview(state, host.as_ref()).await?;
    if let Some(id) = conversation.operation {
        overview.push_str(&operation_section(state, id).await?);
    }
    Ok(ChatContext {
        overview,
        tool_host,
    })
}

/// The "Attached operation" section, or nothing when the operation is gone.
async fn operation_section(
    state: &ApiState,
    id: domovoi_core::OperationId,
) -> Result<String, ApiError> {
    let Some(operation) = state.deps.operation_repo.get(id).await? else {
        return Ok(String::new());
    };
    let operation = operation.value;
    let goal: String = operation.goal.chars().take(GOAL_CHARS).collect();
    let mut text = format!(
        "\nAttached operation (read-only summary):\n- goal: {goal}\n- status: {}\n",
        operation.status.name()
    );
    for subject in &operation.pinned {
        let target = subject
            .target
            .as_ref()
            .map_or(String::new(), |target| format!(" on {target}"));
        text.push_str(&format!(
            "- pinned: {} {}{target}\n",
            subject.host, subject.check
        ));
    }
    Ok(text)
}

/// The text the model is given about the hosts: one line per host with its agent and open
/// incidents, the newest active incidents, and for a chosen host its latest digest facts
/// from the last day.
async fn overview(state: &ApiState, chosen: Option<&Host>) -> Result<String, ApiError> {
    let hosts = state.deps.hosts.list_hosts().await?;
    let mut text = String::from("Hosts:\n");
    for host in &hosts {
        let dto = host_dto(state, host).await?;
        let agent = match (
            dto.connected,
            dto.version.as_deref(),
            dto.connected_since.as_deref(),
        ) {
            (true, Some(version), Some(since)) => {
                format!("connected since {since}, agent {version}")
            }
            (true, Some(version), None) => format!("connected, agent {version}"),
            _ => match dto.last_seen.as_deref() {
                Some(last_seen) => format!("offline, last seen {last_seen}"),
                None => "offline, never seen".to_owned(),
            },
        };
        text.push_str(&format!(
            "- {} ({}): {agent}, {} open incidents\n",
            dto.name, dto.kind, dto.open_incidents
        ));
    }
    let filter = IncidentFilter {
        active: Some(true),
        ..IncidentFilter::default()
    };
    let page = Page {
        cursor: None,
        limit: OVERVIEW_INCIDENTS,
    };
    let active = state.deps.incidents.list(&filter, &page).await?;
    if !active.items.is_empty() {
        text.push_str("\nActive incidents, newest first:\n");
        for incident in &active.items {
            text.push_str(&incident_line(incident, &hosts));
        }
    }
    if let Some(host) = chosen {
        let now = state.deps.clock.now();
        let since = now.checked_sub(FACT_WINDOW).unwrap_or(now);
        // `until` is exclusive; include facts observed right now.
        let until = now.checked_add(SignedDuration::from_secs(1)).unwrap_or(now);
        let facts = state.deps.facts.list(host.id, since, until).await?;
        let lines = fact_lines(&facts);
        if !lines.is_empty() {
            text.push_str(&format!("\nFacts from the last day on {}:\n", host.name));
            text.push_str(&lines);
        }
    }
    Ok(text)
}

/// One line for an active incident in the overview.
fn incident_line(incident: &Incident, hosts: &[Host]) -> String {
    let summary = IncidentSummary::from(incident);
    let host = hosts
        .iter()
        .find(|host| host.id.to_string() == summary.host_id)
        .map_or(summary.host_id.clone(), |host| host.name.to_string());
    let target = summary
        .target
        .as_deref()
        .map_or(String::new(), |target| format!(" on {target}"));
    let verdict = summary
        .verdict
        .as_deref()
        .map_or(String::new(), |verdict| format!(", verdict {verdict}"));
    format!(
        "- [{host}] {}{target}: {} ({}{verdict}), last fired {}\n",
        summary.check, summary.title, summary.status, summary.last_firing_at
    )
}

/// The latest value of each fact, one per line, sorted by key.
fn fact_lines(facts: &[DigestFact]) -> String {
    let mut latest: BTreeMap<&str, &DigestFact> = BTreeMap::new();
    for fact in facts {
        latest.insert(fact.key.as_str(), fact);
    }
    let mut text = String::new();
    for fact in latest.values() {
        let value = match &fact.value {
            FactValue::Text(text) => text.clone(),
            FactValue::Number(n) => ((n * 100.0).round() / 100.0).to_string(),
        };
        let unit = fact
            .unit
            .as_deref()
            .map_or(String::new(), |unit| format!(" {unit}"));
        text.push_str(&format!(
            "- {}: {value}{unit} (observed {})\n",
            fact.label, fact.observed_at
        ));
    }
    text
}
