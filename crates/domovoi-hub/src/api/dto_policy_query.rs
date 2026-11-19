//! Request shapes of the simulator and the recorded-decision list.

use std::str::FromStr;

use domovoi_core::policy_store::{ShadowFilter, ShadowStatus};
use jiff::Timestamp;
use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};

use super::dto_policy_rule::{RuleDto, rules_of};
use super::error::ApiError;
use crate::services::{InputsMode, RulesSource, SelectionChoice, SimulationRequest};

/// Which rules a simulation evaluates.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum RulesSourceDto {
    /// The stored draft.
    Draft,
    /// A published version.
    Version {
        /// The version number.
        number: u32,
    },
    /// Rules sent with the request.
    Inline {
        /// The rules.
        rules: Vec<RuleDto>,
    },
}

/// Which recorded rows a simulation takes.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub struct ShadowFilterDto {
    /// A host id.
    #[serde(default)]
    pub host: Option<String>,
    /// A contract id.
    #[serde(default)]
    pub contract: Option<String>,
    /// `evaluated`, `unevaluable` or `skipped`.
    #[serde(default)]
    pub status: Option<String>,
    /// An enforced outcome word.
    #[serde(default)]
    pub outcome: Option<String>,
    /// Only rows whose enforced and shadow outcomes differ.
    #[serde(default)]
    pub differing: bool,
    /// Only rows recorded at or after this instant, RFC 3339.
    #[serde(default)]
    pub since: Option<String>,
    /// Only rows recorded before this instant, RFC 3339.
    #[serde(default)]
    pub until: Option<String>,
}

impl ShadowFilterDto {
    /// The store filter, or a 400 for a value that does not parse.
    ///
    /// # Errors
    /// `BadRequest` naming the value.
    pub fn filter(&self) -> Result<ShadowFilter, ApiError> {
        let bad = |what: &str| ApiError::BadRequest(format!("{what} is not valid"));
        Ok(ShadowFilter {
            host: self
                .host
                .as_deref()
                .map(|text| text.parse().map_err(|_| bad("host")))
                .transpose()?,
            contract: self.contract.clone(),
            status: self
                .status
                .as_deref()
                .map(|text| ShadowStatus::parse(text).ok_or_else(|| bad("status")))
                .transpose()?,
            enforced_outcome: self.outcome.clone(),
            differing: self.differing,
            since: self
                .since
                .as_deref()
                .map(|text| Timestamp::from_str(text).map_err(|_| bad("since")))
                .transpose()?,
            until: self
                .until
                .as_deref()
                .map(|text| Timestamp::from_str(text).map_err(|_| bad("until")))
                .transpose()?,
        })
    }
}

/// Simulate rules over recorded Execution steps.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct SimulateRequest {
    /// Whose rules.
    pub rules: RulesSourceDto,
    /// `enforced`, `shadow` or `both` (the default).
    #[serde(default)]
    pub selection: Option<String>,
    /// `recorded` (the default) or `current`.
    #[serde(default)]
    pub inputs: Option<String>,
    /// Which recorded rows.
    #[serde(default)]
    pub filter: ShadowFilterDto,
    /// Explicit steps instead of a filter; at most 100.
    #[serde(default)]
    pub execution_ids: Vec<String>,
    /// Rows to take, 1 to 500; default 200.
    #[serde(default)]
    pub limit: Option<u16>,
}

impl SimulateRequest {
    /// The service request, or a 400 for a value that does not parse.
    ///
    /// # Errors
    /// `BadRequest` or `Invalid` with the offending rules.
    pub fn parse(self) -> Result<SimulationRequest, ApiError> {
        let rules = match self.rules {
            RulesSourceDto::Draft => RulesSource::Draft,
            RulesSourceDto::Version { number } => RulesSource::Version(number),
            RulesSourceDto::Inline { rules } => RulesSource::Inline(
                rules_of(rules).map_err(|issues| super::policy::invalid(&issues))?,
            ),
        };
        let selection = match self.selection.as_deref() {
            None | Some("both") => SelectionChoice::Both,
            Some("enforced") => SelectionChoice::Enforced,
            Some("shadow") => SelectionChoice::Shadow,
            Some(_) => return Err(ApiError::BadRequest("selection is not valid".to_owned())),
        };
        let inputs = match self.inputs.as_deref() {
            None | Some("recorded") => InputsMode::Recorded,
            Some("current") => InputsMode::Current,
            Some(_) => return Err(ApiError::BadRequest("inputs is not valid".to_owned())),
        };
        let execution_ids = self
            .execution_ids
            .iter()
            .map(|id| {
                id.parse()
                    .map_err(|_| ApiError::BadRequest("execution id is not valid".to_owned()))
            })
            .collect::<Result<_, _>>()?;
        Ok(SimulationRequest {
            rules,
            selection,
            inputs,
            filter: self.filter.filter()?,
            execution_ids,
            limit: self.limit.unwrap_or(0),
        })
    }
}

/// Filters of the recorded-decision list.
#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ShadowQuery {
    /// A host id.
    pub host: Option<String>,
    /// A contract id.
    pub contract: Option<String>,
    /// `evaluated`, `unevaluable` or `skipped`.
    pub status: Option<String>,
    /// An enforced outcome word.
    pub outcome: Option<String>,
    /// Only rows whose enforced and shadow outcomes differ.
    pub differing: Option<bool>,
    /// RFC 3339 lower bound of the recording time.
    pub since: Option<String>,
    /// RFC 3339 upper bound of the recording time.
    pub until: Option<String>,
    /// The `next_cursor` of the previous page.
    pub cursor: Option<String>,
    /// Page size, 1 to 100. Default 25.
    pub limit: Option<u16>,
}

impl ShadowQuery {
    /// The store filter.
    ///
    /// # Errors
    /// `BadRequest` for a value that does not parse.
    pub fn filter(&self) -> Result<ShadowFilter, ApiError> {
        ShadowFilterDto {
            host: self.host.clone(),
            contract: self.contract.clone(),
            status: self.status.clone(),
            outcome: self.outcome.clone(),
            differing: self.differing.unwrap_or(false),
            since: self.since.clone(),
            until: self.until.clone(),
        }
        .filter()
    }
}
