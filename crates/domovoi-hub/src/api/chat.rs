//! A general chat about the operator's hosts: one host with its read-only tools, or every
//! host from an overview. The model never proposes or runs anything.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::State;
use domovoi_core::ports::{IncidentFilter, Page};
use domovoi_core::{DigestFact, FactValue, Host, Incident};
use domovoi_engine::triage::{ChatRole, ChatTurn, TriageError};
use jiff::SignedDuration;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::dto::IncidentSummary;
use super::error::{ApiError, ErrorBody};
use super::hosts::{FACT_WINDOW, host_dto};
use super::{ApiState, Authed, host_named};

/// The longest question a chat takes, in characters.
pub const MAX_QUESTION_CHARS: usize = 1_000;

/// The most earlier turns a chat replays.
pub const MAX_HISTORY_TURNS: usize = 20;

/// The longest one earlier turn, in characters.
pub const MAX_TURN_CHARS: usize = 4_000;

/// The most active incidents the overview lists.
const OVERVIEW_INCIDENTS: u16 = 20;

/// One earlier turn of the chat.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
pub struct ChatTurnDto {
    /// Who said it: `user` or `assistant`.
    pub role: String,
    /// What was said.
    pub text: String,
}

/// A general question, the host it is about and the conversation so far.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
pub struct ChatRequest {
    /// The host to look at; absent or `null` asks about every host.
    pub host: Option<String>,
    /// The earlier turns, oldest first.
    #[serde(default)]
    pub history: Vec<ChatTurnDto>,
    /// The question to answer.
    pub question: String,
}

/// The model's answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ChatResponse {
    /// The answer, in Markdown.
    pub answer: String,
}

/// Answers a general question about the operator's hosts. With a connected `host` the model
/// may use that host's read-only tools and read skills; without one, or with an offline host,
/// it answers from an overview and calls no tools. It never proposes or runs actions.
#[utoipa::path(
    post, path = "/api/v1/chat", operation_id = "chat", tag = "hosts",
    request_body = ChatRequest,
    responses(
        (status = 200, description = "The answer", body = ChatResponse),
        (status = 400, description = "The question or a turn is empty or too long", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such host", body = ErrorBody),
        (status = 503, description = "Triage is off, the monthly model budget is spent, or the model is unavailable", body = ErrorBody),
    )
)]
pub(super) async fn chat(
    State(state): State<ApiState>,
    _auth: Authed,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, ApiError> {
    let question = req.question.trim();
    if question.is_empty() || question.chars().count() > MAX_QUESTION_CHARS {
        return Err(ApiError::BadRequest(format!(
            "the question must have 1 to {MAX_QUESTION_CHARS} characters"
        )));
    }
    if req.history.len() > MAX_HISTORY_TURNS {
        return Err(ApiError::BadRequest(format!(
            "the history must have at most {MAX_HISTORY_TURNS} turns"
        )));
    }
    let mut history = Vec::with_capacity(req.history.len());
    for turn in &req.history {
        let role = match turn.role.as_str() {
            "user" => ChatRole::User,
            "assistant" => ChatRole::Assistant,
            other => {
                return Err(ApiError::BadRequest(format!(
                    "the role must be `user` or `assistant`, not `{other}`"
                )));
            }
        };
        if turn.text.trim().is_empty() {
            return Err(ApiError::BadRequest(
                "each earlier turn must have at least 1 character".to_owned(),
            ));
        }
        if turn.text.chars().count() > MAX_TURN_CHARS {
            return Err(ApiError::BadRequest(format!(
                "each earlier turn must have at most {MAX_TURN_CHARS} characters"
            )));
        }
        history.push(ChatTurn {
            role,
            text: turn.text.clone(),
        });
    }
    let triage = state.deps.triage.clone().ok_or(ApiError::TriageOff)?;
    let host = match &req.host {
        Some(name) => Some(host_named(&state, name).await?),
        None => None,
    };
    // An offline host has no agent to ask, so the model gets the hub's overview and no
    // tools, the same as "All hosts", rather than tools whose calls would all fail.
    let tool_host = match &host {
        Some(host) if host_dto(&state, host).await?.connected => Some(host.id),
        _ => None,
    };
    let overview = overview(&state, host.as_ref()).await?;
    let question = question.to_owned();
    // A task of its own, so that a closed tab or a navigation does not cancel the model
    // call halfway through the conversation.
    let answer =
        tokio::spawn(async move { triage.chat(tool_host, &overview, &history, &question).await })
            .await
            .map_err(|e| ApiError::internal("chat failed", e))?
            .map_err(|e| match e {
                TriageError::UnknownIncident(_) => ApiError::NotFound,
                TriageError::Provider(err) => ApiError::Unavailable(err),
                TriageError::BudgetSpent => ApiError::BudgetSpent,
                TriageError::Store(err) => ApiError::internal("chat failed", err),
            })?;
    Ok(Json(ChatResponse { answer }))
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
