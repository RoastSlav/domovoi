//! Ack rules: known conditions that never notify.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use domovoi_core::ports::AuditEvent;
use domovoi_core::{
    AckRule, AckRuleId, CheckKey, EvidencePredicate, EvidenceValue, HostId, TargetPattern,
};
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;

use super::error::{ApiError, ErrorBody};
use super::{ApiState, Authed, host_named};
use crate::auth::Current;

/// The longest note of a rule, in characters.
pub const MAX_NOTE_CHARS: usize = 500;

/// The most conditions one rule may require.
pub const MAX_PREDICATES: usize = 10;

/// The longest rule life, in minutes: one year.
pub const MAX_EXPIRY_MINUTES: u32 = 525_600;

/// How a condition compares an evidence value with its `value`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PredicateOpDto {
    /// The evidence value equals it.
    Equals,
    /// Text contains it.
    Contains,
    /// A number is at most it.
    AtMost,
    /// A number is at least it.
    AtLeast,
}

/// One condition on a signal's evidence. `value` is text, a number, `true` or `false`;
/// `at_most` and `at_least` need a number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct PredicateDto {
    /// Evidence key, for example `value` or `user`.
    pub key: String,
    /// How the evidence compares with `value`.
    pub op: PredicateOpDto,
    /// What the evidence must compare with, as text.
    pub value: String,
}

/// A known condition: incidents that match it stay in the history and never notify.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct AckRuleDto {
    /// Rule id.
    pub id: String,
    /// The host the rule is limited to, by name; null means every host. A host missing
    /// from the store shows as `unknown host` with its id, so the list still works.
    pub host: Option<String>,
    /// The check the rule matches, for example `security.ssh_login`.
    pub check: String,
    /// Target pattern with `*` allowed at the ends; null means any target.
    pub target_pattern: Option<String>,
    /// Conditions, all of which must hold.
    pub predicates: Vec<PredicateDto>,
    /// Why the condition is acceptable.
    pub note: String,
    /// When the rule stops applying; null means it never expires.
    pub expires_at: Option<String>,
}

/// A new rule.
#[derive(Debug, Clone, PartialEq, Deserialize, ToSchema)]
pub struct AckRuleInput {
    /// Host by name; null matches every host.
    pub host: Option<String>,
    /// The check to match, for example `security.ssh_login`.
    pub check: String,
    /// Target pattern with `*` allowed at the ends; null matches any target.
    pub target_pattern: Option<String>,
    /// Conditions, all of which must hold; at most 10.
    pub predicates: Vec<PredicateDto>,
    /// Why the condition is acceptable, 1 to 500 characters.
    pub note: String,
    /// Minutes from now until the rule stops applying; null means it never expires.
    pub expires_in_minutes: Option<u32>,
}

fn parse_id(id: &str) -> Result<AckRuleId, ApiError> {
    id.parse().map_err(|_| ApiError::NotFound)
}

fn op_name(op: PredicateOpDto) -> &'static str {
    match op {
        PredicateOpDto::Equals => "equals",
        PredicateOpDto::Contains => "contains",
        PredicateOpDto::AtMost => "at_most",
        PredicateOpDto::AtLeast => "at_least",
    }
}

/// A validated evidence key: 1 to 64 characters of `a-z`, `0-9` and `_`.
fn evidence_key(key: &str) -> Result<String, ApiError> {
    let key = key.trim();
    if key.is_empty() || key.len() > 64 {
        return Err(ApiError::BadRequest(
            "the evidence key must have 1 to 64 characters".to_owned(),
        ));
    }
    if !key
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return Err(ApiError::BadRequest(
            "the evidence key uses only a-z, 0-9 and underscore".to_owned(),
        ));
    }
    Ok(key.to_owned())
}

fn predicate_to_core(dto: PredicateDto) -> Result<EvidencePredicate, ApiError> {
    let key = evidence_key(&dto.key)?;
    let value = dto.value.trim();
    match dto.op {
        PredicateOpDto::Equals => {
            let value = if let Ok(int) = value.parse::<i64>() {
                EvidenceValue::Int(int)
            } else if let Ok(float) = value.parse::<f64>() {
                EvidenceValue::Float(float)
            } else if matches!(value, "true" | "false") {
                EvidenceValue::Bool(value == "true")
            } else {
                EvidenceValue::Text(value.to_owned())
            };
            Ok(EvidencePredicate::Equals { key, value })
        }
        PredicateOpDto::Contains => {
            if value.is_empty() {
                return Err(ApiError::BadRequest(
                    "contains needs something to look for".to_owned(),
                ));
            }
            Ok(EvidencePredicate::Contains {
                key,
                text: value.to_owned(),
            })
        }
        PredicateOpDto::AtMost | PredicateOpDto::AtLeast => {
            let bound = value.parse::<f64>().map_err(|_| {
                ApiError::BadRequest(format!(
                    "the value of a {} condition is a number",
                    op_name(dto.op)
                ))
            })?;
            if !bound.is_finite() {
                return Err(ApiError::BadRequest(format!(
                    "the value of a {} condition is a number",
                    op_name(dto.op)
                )));
            }
            match dto.op {
                PredicateOpDto::AtMost => Ok(EvidencePredicate::LessOrEqual { key, value: bound }),
                _ => Ok(EvidencePredicate::GreaterOrEqual { key, value: bound }),
            }
        }
    }
}

fn predicate_to_dto(predicate: &EvidencePredicate) -> PredicateDto {
    let (key, op, value) = match predicate {
        EvidencePredicate::Equals { key, value } => (
            key.clone(),
            PredicateOpDto::Equals,
            match value {
                EvidenceValue::Int(int) => int.to_string(),
                EvidenceValue::Float(float) => float.to_string(),
                EvidenceValue::Bool(flag) => flag.to_string(),
                EvidenceValue::Text(text) => text.clone(),
            },
        ),
        EvidencePredicate::Contains { key, text } => {
            (key.clone(), PredicateOpDto::Contains, text.clone())
        }
        EvidencePredicate::LessOrEqual { key, value } => {
            (key.clone(), PredicateOpDto::AtMost, value.to_string())
        }
        EvidencePredicate::GreaterOrEqual { key, value } => {
            (key.clone(), PredicateOpDto::AtLeast, value.to_string())
        }
    };
    PredicateDto { key, op, value }
}

/// The rule as a DTO, with the host's name. A host that is not in the store shows as
/// `unknown host` with its id, so a hand-edited database cannot make the whole list fail.
async fn rule_to_dto(state: &ApiState, rule: &AckRule) -> Result<AckRuleDto, ApiError> {
    let host = match rule.host {
        Some(id) => {
            let host = state.deps.hosts.get_host(id).await?;
            Some(
                host.map(|host| host.name.to_string())
                    .unwrap_or(format!("unknown host {id}")),
            )
        }
        None => None,
    };
    Ok(AckRuleDto {
        id: rule.id.to_string(),
        host,
        check: rule.check.as_str().to_owned(),
        target_pattern: rule.target_pattern.as_ref().map(|p| p.to_string()),
        predicates: rule.predicates.iter().map(predicate_to_dto).collect(),
        note: rule.note.clone(),
        expires_at: rule.expires_at.map(|at| at.to_string()),
    })
}

/// The host named in `name`, by name; null is no limit.
async fn host_limit(state: &ApiState, name: Option<&String>) -> Result<Option<HostId>, ApiError> {
    match name {
        Some(name) => {
            let host = host_named(state, name).await.map_err(|err| match err {
                ApiError::NotFound => ApiError::BadRequest(format!("unknown host {name}")),
                other => other,
            })?;
            Ok(Some(host.id))
        }
        None => Ok(None),
    }
}

/// Every rule, expired ones included. Rules that never expire come last.
#[utoipa::path(
    get, path = "/api/v1/ack-rules", operation_id = "list_ack_rules", tag = "incidents",
    responses(
        (status = 200, description = "Every rule, soonest expiry first; rules that never expire come last", body = Vec<AckRuleDto>),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(super) async fn list(
    State(state): State<ApiState>,
    _auth: Authed,
) -> Result<Json<Vec<AckRuleDto>>, ApiError> {
    let mut rules = state.deps.acks.list_rules().await?;
    rules.sort_by_key(|rule| rule.expires_at.unwrap_or(Timestamp::MAX));
    let mut dtos = Vec::with_capacity(rules.len());
    for rule in &rules {
        dtos.push(rule_to_dto(&state, rule).await?);
    }
    Ok(Json(dtos))
}

/// Audits that `actor` created `rule` at `at`. The hub creates rules from the ack-rule
/// page and from dismissing an incident, so both write the same event.
pub(crate) async fn audit_rule_created(
    state: &ApiState,
    actor: &Current,
    rule: &AckRule,
    at: Timestamp,
) -> Result<(), ApiError> {
    state
        .deps
        .audit
        .append(&AuditEvent {
            at,
            actor: format!("user:{}", actor.user.id),
            kind: "ack_rule.created".to_owned(),
            subject: format!("ack_rule:{}", rule.id),
            detail: json!({
                "check": rule.check.as_str(),
                "note": rule.note,
                "expires_at": rule.expires_at.map(|at| at.to_string()),
            }),
        })
        .await?;
    Ok(())
}

/// Creates a rule.
#[utoipa::path(
    post, path = "/api/v1/ack-rules", operation_id = "create_ack_rule", tag = "incidents",
    request_body = AckRuleInput,
    responses(
        (status = 201, description = "Created", body = AckRuleDto),
        (status = 400, description = "Invalid rule", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(super) async fn create(
    State(state): State<ApiState>,
    Authed(current): Authed,
    Json(input): Json<AckRuleInput>,
) -> Result<(StatusCode, Json<AckRuleDto>), ApiError> {
    let check = CheckKey::parse(&input.check).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let target_pattern = match &input.target_pattern {
        Some(pattern) => {
            Some(TargetPattern::parse(pattern).map_err(|e| ApiError::BadRequest(e.to_string()))?)
        }
        None => None,
    };
    let note = input.note.trim();
    if note.is_empty() || note.chars().count() > MAX_NOTE_CHARS {
        return Err(ApiError::BadRequest(format!(
            "the note must have 1 to {MAX_NOTE_CHARS} characters"
        )));
    }
    if input.predicates.len() > MAX_PREDICATES {
        return Err(ApiError::BadRequest(format!(
            "a rule has at most {MAX_PREDICATES} conditions"
        )));
    }
    let predicates = input
        .predicates
        .into_iter()
        .map(predicate_to_core)
        .collect::<Result<Vec<_>, _>>()?;
    let host = host_limit(&state, input.host.as_ref()).await?;
    let now = state.deps.clock.now();
    let expires_at = match input.expires_in_minutes {
        Some(mins) => {
            if !(1..=MAX_EXPIRY_MINUTES).contains(&mins) {
                return Err(ApiError::BadRequest(format!(
                    "minutes must be between 1 and {MAX_EXPIRY_MINUTES}"
                )));
            }
            Some(
                now.checked_add(SignedDuration::from_mins(i64::from(mins)))
                    .map_err(|e| ApiError::BadRequest(e.to_string()))?,
            )
        }
        None => None,
    };
    let rule = AckRule {
        id: AckRuleId::new(),
        host,
        check,
        target_pattern,
        predicates,
        note: note.to_owned(),
        expires_at,
    };
    state.deps.acks.put_rule(&rule).await?;
    audit_rule_created(&state, &current, &rule, now).await?;
    Ok((StatusCode::CREATED, Json(rule_to_dto(&state, &rule).await?)))
}

/// Deletes a rule.
#[utoipa::path(
    delete, path = "/api/v1/ack-rules/{id}", operation_id = "delete_ack_rule", tag = "incidents",
    params(("id" = String, Path, description = "Rule id")),
    responses(
        (status = 204, description = "Deleted"),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such rule", body = ErrorBody),
    )
)]
pub(super) async fn delete(
    State(state): State<ApiState>,
    Authed(current): Authed,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = parse_id(&id)?;
    let rule = state
        .deps
        .acks
        .list_rules()
        .await?
        .into_iter()
        .find(|rule| rule.id == id)
        .ok_or(ApiError::NotFound)?;
    state.deps.acks.delete_rule(id).await?;
    state
        .deps
        .audit
        .append(&AuditEvent {
            at: state.deps.clock.now(),
            actor: format!("user:{}", current.user.id),
            kind: "ack_rule.deleted".to_owned(),
            subject: format!("ack_rule:{}", rule.id),
            detail: json!({ "check": rule.check.as_str(), "note": rule.note }),
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
