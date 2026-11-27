//! The read-only view of one host's unattended ceiling, exactly as its agent reported it.
//!
//! The ceiling belongs to the host: this module reads the hub's memory of the agent's
//! summary and projects it. It writes nothing, calls no agent and reads no settings.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, State};
use domovoi_core::capability::{CeilingEntry, CeilingTargets};
use serde::Serialize;
use utoipa::ToSchema;

use super::error::{ApiError, ErrorBody};
use super::{ApiState, Authed, host_named};
use crate::capability::CeilingState;

/// Which targets a ceiling entry covers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CeilingTargetsDto {
    /// Every target of the contract.
    Any,
    /// Only these targets, sorted ascending.
    Names {
        /// The covered target names.
        names: Vec<String>,
    },
}

/// One capability's bound in the host's unattended ceiling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct CeilingEntryDto {
    /// The exact contract id.
    pub contract: String,
    /// The targets the entry covers.
    pub targets: CeilingTargetsDto,
    /// The most runs allowed in any hour.
    pub max_per_hour: u32,
    /// How many runs may be active at once, when the entry sets a limit.
    pub max_active: Option<u32>,
    /// The longest hold in seconds, when the entry sets a limit.
    pub max_ttl_secs: Option<i64>,
    /// Argument bounds, key to integer bound.
    pub args: BTreeMap<String, i64>,
    /// Whether the helper on this host would serve it. The ceiling is only one of the three
    /// decisions (the agent's limit, the helper's offer, a policy rule); a row can be allowed
    /// here and still end in a helper refusal.
    pub helper: HelperOfferDto,
}

/// What the host's helper says about a capability that runs through it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum HelperOfferDto {
    /// The capability does not run through the helper.
    NotApplicable,
    /// The helper offers it.
    Offered,
    /// The helper does not offer it, and why, with the file that decides it.
    NotOffered {
        /// The availability reason code, for example `helper_systemd_group_missing`.
        reason: String,
        /// The reason in words: what to change in `/etc/domovoi/helper.toml`.
        text: String,
    },
}

impl From<&CeilingEntry> for CeilingEntryDto {
    fn from(entry: &CeilingEntry) -> Self {
        Self {
            contract: entry.contract.to_string(),
            targets: match &entry.targets {
                CeilingTargets::Any => CeilingTargetsDto::Any,
                CeilingTargets::Names(names) => {
                    let mut names: Vec<String> = names.iter().map(ToString::to_string).collect();
                    names.sort();
                    CeilingTargetsDto::Names { names }
                }
            },
            max_per_hour: entry.max_per_hour,
            max_active: entry.max_active,
            max_ttl_secs: entry.max_ttl.map(|ttl| ttl.as_secs()),
            args: entry.args.clone(),
            helper: HelperOfferDto::NotApplicable,
        }
    }
}

/// How the host's ceiling summary reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CeilingReporting {
    /// A valid summary arrived.
    Reported,
    /// No summary reached the hub.
    NotReported,
    /// A summary arrived and could not be read; it is treated as not permissive.
    Unreadable,
}

/// What one host reported as its unattended limits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct CeilingDto {
    /// The host's name.
    pub host: String,
    /// Whether the host has a live agent session now.
    pub connected: bool,
    /// How the summary reads.
    pub reporting: CeilingReporting,
    /// `[unattended].enabled` on the host; only set when reported.
    pub unattended_enabled: Option<bool>,
    /// The agent's opaque configuration fingerprint, only when reported.
    pub hash: Option<String>,
    /// One entry per capability the host lets run unattended, sorted by contract.
    pub entries: Vec<CeilingEntryDto>,
    /// Why the summary could not be read; only set when unreadable.
    pub unreadable_reason: Option<String>,
}

/// The unattended limits host `name` reported. Read only.
#[utoipa::path(
    get, path = "/api/v1/hosts/{name}/ceiling", operation_id = "get_host_ceiling",
    tag = "hosts",
    params(("name" = String, Path, description = "Host name")),
    responses(
        (status = 200, description = "What the host reported", body = CeilingDto),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such host", body = ErrorBody),
    )
)]
pub(super) async fn get_ceiling(
    State(state): State<ApiState>,
    _auth: Authed,
    Path(name): Path<String>,
) -> Result<Json<CeilingDto>, ApiError> {
    let host = host_named(&state, &name).await?;
    let report = state.deps.capabilities.ceiling(host.id);
    // The units the helper may restart, only from a verified report: an unverified helper
    // says nothing about its list, so nothing is claimed.
    let helper_units = state
        .services()
        .helper_trust
        .view(host.name.clone())
        .await
        .ok()
        .and_then(|view| view.detail.verified.map(|v| v.restart_units));
    let mut dto = CeilingDto {
        host: host.name.to_string(),
        connected: report.connected,
        reporting: CeilingReporting::NotReported,
        unattended_enabled: None,
        hash: None,
        entries: Vec::new(),
        unreadable_reason: None,
    };
    match report.state {
        CeilingState::NotReported => {}
        CeilingState::Unreadable(reason) => {
            dto.reporting = CeilingReporting::Unreadable;
            dto.unreadable_reason = Some(reason.chars().take(200).collect());
        }
        CeilingState::Reported(summary) => {
            let mut entries: Vec<CeilingEntryDto> = summary
                .entries
                .iter()
                .map(|entry| {
                    let mut row = CeilingEntryDto::from(entry);
                    row.helper = match state
                        .deps
                        .capabilities
                        .helper_offer(host.id, &entry.contract)
                    {
                        None => HelperOfferDto::NotApplicable,
                        Some(Ok(())) => restart_list_refusal(entry, helper_units.as_deref())
                            .unwrap_or(HelperOfferDto::Offered),
                        Some(Err(reason)) => HelperOfferDto::NotOffered {
                            reason: reason.code().to_owned(),
                            text: reason.describe().to_owned(),
                        },
                    };
                    row
                })
                .collect();
            entries.sort_by(|a, b| a.contract.cmp(&b.contract));
            dto.reporting = CeilingReporting::Reported;
            dto.unattended_enabled = Some(summary.enabled);
            dto.hash = report.hash.or(summary.hash);
            dto.entries = entries;
        }
    }
    Ok(Json(dto))
}

/// `unit.restart` is allowed by the ceiling for units the helper's own list leaves out: the
/// helper would refuse them. Only a verified list is read.
fn restart_list_refusal(entry: &CeilingEntry, listed: Option<&[String]>) -> Option<HelperOfferDto> {
    if entry.contract.as_str() != "unit.restart" {
        return None;
    }
    let listed = listed?;
    let full = |unit: &str| {
        if unit.contains('.') {
            unit.to_owned()
        } else {
            format!("{unit}.service")
        }
    };
    let missing: Vec<String> = match &entry.targets {
        CeilingTargets::Any if listed.is_empty() => vec!["any unit".to_owned()],
        CeilingTargets::Any => Vec::new(),
        CeilingTargets::Names(names) => names
            .iter()
            .map(|name| name.to_string())
            .filter(|name| !listed.iter().any(|unit| full(unit) == full(name)))
            .collect(),
    };
    if missing.is_empty() {
        return None;
    }
    Some(HelperOfferDto::NotOffered {
        reason: "helper_unit_not_listed".to_owned(),
        text: format!(
            "The helper's list does not include {}. Add it to `[systemd] restart_units` in /etc/domovoi/helper.toml on the host.",
            missing.join(", ")
        ),
    })
}

#[cfg(test)]
mod restart_list_tests {
    use std::collections::{BTreeMap, BTreeSet};

    use domovoi_core::capability::{CeilingEntry, CeilingTargets, ContractId, TargetName};

    use super::{HelperOfferDto, restart_list_refusal};

    fn entry(contract: &str, targets: CeilingTargets) -> CeilingEntry {
        CeilingEntry {
            contract: ContractId::parse(contract).unwrap(),
            targets,
            max_per_hour: 2,
            max_active: None,
            max_ttl: None,
            args: BTreeMap::new(),
        }
    }

    fn names(list: &[&str]) -> CeilingTargets {
        CeilingTargets::Names(
            list.iter()
                .map(|n| TargetName::parse(n).unwrap())
                .collect::<BTreeSet<_>>(),
        )
    }

    #[test]
    fn a_unit_the_helper_does_not_list_is_named_with_the_file_to_change() {
        let entry = entry(
            "unit.restart",
            names(&["smartmontools.service", "nginx.service"]),
        );
        let listed = ["nginx".to_owned()];
        let Some(HelperOfferDto::NotOffered { reason, text }) =
            restart_list_refusal(&entry, Some(&listed))
        else {
            panic!("a missing unit is refused");
        };
        assert_eq!(reason, "helper_unit_not_listed");
        assert!(text.contains("smartmontools.service"), "{text}");
        assert!(!text.contains("nginx.service,"), "{text}");
        assert!(text.contains("restart_units"), "{text}");
    }

    #[test]
    fn nothing_is_claimed_without_a_verified_list_or_for_other_contracts() {
        let restart = entry("unit.restart", names(&["a.service"]));
        assert!(restart_list_refusal(&restart, None).is_none());
        let other = entry("firewall.block_source", CeilingTargets::Any);
        assert!(restart_list_refusal(&other, Some(&[])).is_none());
        let listed = ["a.service".to_owned()];
        assert!(restart_list_refusal(&restart, Some(&listed)).is_none());
    }
}
