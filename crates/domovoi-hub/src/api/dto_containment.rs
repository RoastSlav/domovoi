//! The containment endpoints' request and response bodies.
//!
//! A body never holds a key, a path on the hub's disk or a journal line.

use domovoi_core::ports::BlockRow;
use domovoi_engine::containment::HostContainment;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::services::{ContainmentHostView, ContainmentPreview};

/// The leak self-test gate the helper reported at the last refresh.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct GateDto {
    /// `open`, `blocked_leak`, `blocked_untested` or `unknown` before a refresh.
    pub state: String,
    /// Why, when the helper said.
    pub reason: Option<String>,
    /// The layout the sets use: `hosts` or `hosts_and_prefixes`.
    pub layout: Option<String>,
}

/// The drills the helper reported at the last refresh.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct DrillsDto {
    /// `passed`, `failed`, `not_run` or `unknown` before a refresh.
    pub state: String,
}

/// One block the hub recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct BlockDto {
    /// The block's id: the execution id of its apply.
    pub block_id: String,
    /// The host.
    pub host_id: String,
    /// The operation the block belongs to.
    pub operation_id: String,
    /// The step the block belongs to.
    pub step_id: String,
    /// The canonical network.
    pub source: String,
    /// The TTL the block has once confirmed, in seconds.
    pub ttl_s: u32,
    /// `applying`, `applied`, `confirming`, `confirmed`, `rolling_back`, `rolled_back`,
    /// `expired`, `lost`, `not_run`, `failed` or `outcome_unknown`.
    pub phase: String,
    /// `human` or `policy`.
    pub authorization: String,
    /// `verified` or `unverified`.
    pub assurance: String,
    /// Why coverage could not be verified, on the unverified path.
    pub reason: Option<String>,
    /// The paths the verified report did not cover, on the unverified path.
    pub unprotected_paths: Vec<String>,
    /// When the helper applied the block.
    pub applied_at: Option<String>,
    /// When a confirmed block ends.
    pub deadline: Option<String>,
    /// `cut`, `n/a` or `may_persist`.
    pub existing_flows: Option<String>,
    /// Whether a bridged interface leaves a gap.
    pub bridge_gap: Option<bool>,
    /// The token of a refusal or failure.
    pub failure_token: Option<String>,
    /// When the block was created.
    pub created_at: String,
    /// When the block last changed.
    pub updated_at: String,
}

fn assurance_parts(text: &str) -> (String, Option<String>, Vec<String>) {
    let value: serde_json::Value = serde_json::from_str(text).unwrap_or_default();
    let kind = value
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown")
        .to_owned();
    let reason = value
        .get("reason")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let paths = value
        .get("unprotected_paths")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    (kind, reason, paths)
}

impl From<&BlockRow> for BlockDto {
    fn from(row: &BlockRow) -> Self {
        let (assurance, reason, unprotected_paths) = assurance_parts(&row.assurance);
        Self {
            block_id: row.block_id.to_string(),
            host_id: row.host_id.to_string(),
            operation_id: row.operation_id.to_string(),
            step_id: row.step_id.to_string(),
            source: row.source.clone(),
            ttl_s: row.ttl_s,
            phase: row.phase.as_str().to_owned(),
            authorization: row.authorization_kind.clone(),
            assurance,
            reason,
            unprotected_paths,
            applied_at: row.applied_at.map(|t| t.to_string()),
            deadline: row.deadline.map(|t| t.to_string()),
            existing_flows: row.existing_flows.clone(),
            bridge_gap: row.bridge_gap,
            failure_token: row.failure_token.clone(),
            created_at: row.created_at.to_string(),
            updated_at: row.updated_at.to_string(),
        }
    }
}

/// What a host's containment looks like: display only, and available when containment is not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ContainmentStatusDto {
    /// Whether the registry offers containment on this host right now.
    pub available: bool,
    /// The leak self-test gate.
    pub gate: GateDto,
    /// The drills.
    pub drills: DrillsDto,
    /// The blocks the hub recorded, newest first.
    pub blocks: Vec<BlockDto>,
    /// When the helper was last asked (a refresh by the owner); absent before one.
    pub observed_at: Option<String>,
    /// Blocks the helper lost with its table, when it said.
    pub lost_blocks: Option<String>,
    /// Whether a bridged interface leaves a gap.
    pub bridge_gap: Option<bool>,
    /// `cut`, `n/a` or `may_persist`.
    pub existing_flows: Option<String>,
}

impl ContainmentStatusDto {
    /// The status of a host, with whether the registry offers containment there.
    #[must_use]
    pub fn of(view: &ContainmentHostView, available: bool) -> Self {
        let seen: Option<&HostContainment> = view.status.as_ref();
        let gate_text = seen.map_or("", |s| s.gate.as_str());
        let (state, reason) = match gate_text.split_once(": ") {
            Some((state, reason)) => (state.to_owned(), Some(reason.to_owned())),
            None if gate_text.is_empty() => ("unknown".to_owned(), None),
            None => (gate_text.to_owned(), None),
        };
        Self {
            available,
            gate: GateDto {
                state,
                reason,
                layout: seen.and_then(|s| s.layout.clone()),
            },
            drills: DrillsDto {
                state: seen
                    .map(|s| s.drills.as_str())
                    .filter(|d| !d.is_empty())
                    .unwrap_or("unknown")
                    .to_owned(),
            },
            blocks: view.blocks.iter().map(BlockDto::from).collect(),
            observed_at: seen.and_then(|s| s.observed_at).map(|t| t.to_string()),
            lost_blocks: seen.and_then(|s| s.lost_blocks.clone()),
            bridge_gap: seen.and_then(|s| s.bridge_gap),
            existing_flows: seen.and_then(|s| s.existing_flows.clone()),
        }
    }
}

/// Releasing a block early: asks for the password again and the source typed out.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub struct ReleaseRequest {
    /// The signed-in person's password.
    pub password: Option<String>,
    /// The block's canonical source, typed exactly.
    pub confirm_source: Option<String>,
}

/// How a containment step stands, on the step detail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ContainmentStepDto {
    /// A block phase, or `proposed` before the step was dispatched.
    pub phase: String,
    /// The canonical source.
    pub source: String,
    /// The TTL in seconds.
    pub ttl_s: i64,
    /// `verified` or `unverified`.
    pub assurance: String,
    /// Why coverage could not be verified, on the unverified path.
    pub reason: Option<String>,
    /// The paths the verified report does not cover, on the unverified path.
    pub unprotected_paths: Vec<String>,
    /// `cut`, `n/a` or `may_persist`.
    pub existing_flows: Option<String>,
    /// Whether a bridged interface leaves a gap.
    pub bridge_gap: Option<bool>,
    /// When a confirmed block ends.
    pub expires_at: Option<String>,
}

impl ContainmentStepDto {
    /// The step of a recorded block.
    #[must_use]
    pub fn of_block(row: &BlockRow) -> Self {
        let dto = BlockDto::from(row);
        Self {
            phase: dto.phase,
            source: dto.source,
            ttl_s: i64::from(dto.ttl_s),
            assurance: dto.assurance,
            reason: dto.reason,
            unprotected_paths: dto.unprotected_paths,
            existing_flows: dto.existing_flows,
            bridge_gap: dto.bridge_gap,
            expires_at: dto.deadline,
        }
    }

    /// The step before it was dispatched: what the approval would show now.
    #[must_use]
    pub fn of_preview(preview: &ContainmentPreview) -> Self {
        Self {
            phase: "proposed".to_owned(),
            source: preview.source.clone(),
            ttl_s: preview.ttl_s,
            assurance: if preview.unverified_reason.is_some() {
                "unverified"
            } else {
                "verified"
            }
            .to_owned(),
            reason: preview.unverified_reason.map(str::to_owned),
            unprotected_paths: preview.unprotected.clone(),
            existing_flows: None,
            bridge_gap: None,
            expires_at: None,
        }
    }
}
