//! Editing a known condition, and the check names a rule may name.

use axum::Json;
use axum::extract::{Path, State};
use domovoi_core::CheckKey;
use domovoi_core::ports::{AuditEvent, IncidentFilter, Page};
use serde::{Deserialize, Deserializer};
use serde_json::json;
use utoipa::ToSchema;

use super::error::{ApiError, ErrorBody};
use super::rules::{
    AckRuleDto, MAX_PREDICATES, PredicateDto, host_limit, parse_expiry, parse_id, parse_note,
    parse_pattern, predicate_to_core, rule_to_dto,
};
use super::{ApiState, Authed};

/// The agent checks this build ships. A rule may also name a check an agent reports, a probe,
/// a webhook source, or one an incident has been raised for.
const BUILT_IN_CHECKS: [&str; 21] = [
    "container.expected_down",
    "container.new",
    "container.restart_loop",
    "container.unhealthy",
    "disk.free",
    "kernel.errors",
    "memory.available",
    "power.supply",
    "security.fail2ban",
    "security.new_port",
    "security.ssh_login",
    "security.sudo_failure",
    "smart.health",
    "system.reboot",
    "system.reboot_required",
    "temperature.cpu",
    "unit.failed",
    "unit.watch",
    "probe.gateway",
    "probe.http",
    "probe.tcp",
];

/// Check families whose names depend on what is configured, so any name in them is accepted.
const OPEN_PREFIXES: [&str; 3] = ["alertmanager.", "uptime_kuma", "hub."];

/// Refuses a check name nothing in this installation could ever raise, so a typo does not
/// become a rule that silently matches nothing.
pub(super) async fn require_known_check(
    state: &ApiState,
    check: &CheckKey,
) -> Result<(), ApiError> {
    let name = check.as_str();
    if BUILT_IN_CHECKS.contains(&name) || OPEN_PREFIXES.iter().any(|p| name.starts_with(p)) {
        return Ok(());
    }
    for host in state.deps.hosts.list_hosts().await? {
        if state
            .deps
            .agents
            .checks(host.id)
            .iter()
            .any(|info| info.key.as_str() == name)
        {
            return Ok(());
        }
    }
    if state
        .deps
        .acks
        .list_rules()
        .await?
        .iter()
        .any(|rule| rule.check.as_str() == name)
    {
        return Ok(());
    }
    let seen = state
        .deps
        .incidents
        .list(&IncidentFilter::default(), &Page::first(Page::MAX_LIMIT))
        .await?;
    if seen.items.iter().any(|i| i.check().as_str() == name) {
        return Ok(());
    }
    Err(ApiError::BadRequest(format!(
        "no check is named {name}: no agent reports it and no incident was raised for it; \
         check the spelling (the checks page of a host lists its checks)"
    )))
}

/// A field that may be absent (leave it), null (clear it) or a value (set it).
fn nullable<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// The fields of a known condition that can change. Anything left out stays as it is.
#[derive(Debug, Clone, PartialEq, Deserialize, ToSchema)]
pub struct AckRulePatch {
    /// Host by name; null makes the rule apply to every host.
    #[serde(default, deserialize_with = "nullable")]
    #[schema(value_type = Option<String>, nullable)]
    pub host: Option<Option<String>>,
    /// Target pattern with `*` allowed at the ends; null matches any target.
    #[serde(default, deserialize_with = "nullable")]
    #[schema(value_type = Option<String>, nullable)]
    pub target_pattern: Option<Option<String>>,
    /// Why the condition is acceptable, 1 to 500 characters.
    #[serde(default)]
    pub note: Option<String>,
    /// Replaces the conditions on the evidence; at most 10.
    #[serde(default)]
    pub predicates: Option<Vec<PredicateDto>>,
    /// The exact time the rule stops applying, as RFC 3339; null means it never expires.
    #[serde(default, deserialize_with = "nullable")]
    #[schema(value_type = Option<String>, nullable)]
    pub expires_at: Option<Option<String>>,
    /// Minutes from now until the rule stops applying. Ignored when `expires_at` is given.
    #[serde(default)]
    pub expires_in_minutes: Option<u32>,
}

/// Edits a rule: its host, target pattern, note, conditions or expiry. The check cannot change;
/// delete the rule and make a new one for that. The same checks as on create apply.
#[utoipa::path(
    patch, path = "/api/v1/ack-rules/{id}", operation_id = "update_ack_rule", tag = "incidents",
    params(("id" = String, Path, description = "Rule id")),
    request_body = AckRulePatch,
    responses(
        (status = 200, description = "Updated", body = AckRuleDto),
        (status = 400, description = "Invalid change", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such rule", body = ErrorBody),
    )
)]
pub(super) async fn update(
    State(state): State<ApiState>,
    Authed(current): Authed,
    Path(id): Path<String>,
    Json(patch): Json<AckRulePatch>,
) -> Result<Json<AckRuleDto>, ApiError> {
    let id = parse_id(&id)?;
    let mut rule = state
        .deps
        .acks
        .list_rules()
        .await?
        .into_iter()
        .find(|rule| rule.id == id)
        .ok_or(ApiError::NotFound)?;
    let now = state.deps.clock.now();
    let mut changed = Vec::new();
    if let Some(host) = &patch.host {
        rule.host = host_limit(&state, host.as_ref()).await?;
        changed.push("host");
    }
    if let Some(pattern) = &patch.target_pattern {
        rule.target_pattern = parse_pattern(pattern.as_deref())?;
        changed.push("target_pattern");
    }
    if let Some(note) = &patch.note {
        rule.note = parse_note(note)?;
        changed.push("note");
    }
    if let Some(predicates) = patch.predicates {
        if predicates.len() > MAX_PREDICATES {
            return Err(ApiError::BadRequest(format!(
                "a rule has at most {MAX_PREDICATES} conditions"
            )));
        }
        rule.predicates = predicates
            .into_iter()
            .map(predicate_to_core)
            .collect::<Result<_, _>>()?;
        changed.push("predicates");
    }
    match (&patch.expires_at, patch.expires_in_minutes) {
        (Some(None), _) => {
            rule.expires_at = None;
            changed.push("expires_at");
        }
        (Some(Some(text)), _) => {
            rule.expires_at = parse_expiry(Some(text), None, now)?;
            changed.push("expires_at");
        }
        (None, Some(mins)) => {
            rule.expires_at = parse_expiry(None, Some(mins), now)?;
            changed.push("expires_at");
        }
        (None, None) => {}
    }
    if changed.is_empty() {
        return Err(ApiError::BadRequest(
            "nothing to change: give at least one of host, target_pattern, note, predicates \
             or expires_at"
                .to_owned(),
        ));
    }
    state.deps.acks.put_rule(&rule).await?;
    state
        .deps
        .audit
        .append(&AuditEvent {
            at: now,
            actor: format!("user:{}", current.user.id),
            kind: "ack_rule.updated".to_owned(),
            subject: format!("ack_rule:{}", rule.id),
            detail: json!({
                "check": rule.check.as_str(),
                "changed": changed,
                "note": rule.note,
                "expires_at": rule.expires_at.map(|at| at.to_string()),
            }),
        })
        .await?;
    Ok(Json(rule_to_dto(&state, &rule).await?))
}
