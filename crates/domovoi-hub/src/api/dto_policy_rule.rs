//! Wire shapes of the policy editor: rules, versions, the draft, previews and publication.
//!
//! [`RuleDto`] is the editor's own rule shape. It is independent of the stored envelope and of
//! the evaluator's serde attributes, so neither can change what a client sees by accident.

use std::collections::BTreeSet;

use domovoi_core::HostId;
use domovoi_core::capability::{
    ConstraintKind, ContractCategory, ContractId, EffectClass, FactId, TargetName,
};
use domovoi_engine::policy::{
    CapabilityScope, Condition, HostScope, Outcome, Rule, RuleId, TargetScope, TimeOfDay,
};
use jiff::SignedDuration;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::services::RuleIssue;

/// The host dimension of a rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HostScopeDto {
    /// Every host.
    All,
    /// One host.
    Host {
        /// The host's id.
        host_id: String,
    },
}

/// The capability dimension of a rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PolicyCapabilityDto {
    /// Every capability.
    Any,
    /// Every capability of one effect class (`read`, `mutate`, `contain`, `destructive`).
    Effect {
        /// The effect class.
        effect: String,
    },
    /// Every capability of one category.
    Category {
        /// The category.
        category: String,
    },
    /// One contract.
    Contract {
        /// The contract id.
        contract: String,
    },
}

/// The target dimension of a rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TargetsDto {
    /// Every target.
    Any,
    /// Only the named targets.
    Exact {
        /// The names.
        names: Vec<String>,
    },
}

/// A condition on a rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConditionDto {
    /// An integer argument bound.
    ArgConstraint {
        /// The argument name.
        arg: String,
        /// `int_at_least` or `int_at_most`.
        op: String,
        /// The bound.
        value: i64,
    },
    /// A UTC time-of-day window; an earlier end wraps across midnight.
    TimeWindow {
        /// `HH:MM`, inclusive.
        start: String,
        /// `HH:MM`, exclusive.
        end: String,
    },
    /// A safety fact that must be known to have a value.
    RequiresFact {
        /// The fact id.
        fact: String,
        /// The value it must be known to have.
        expected: bool,
    },
    /// A request-rate bound.
    MaxRate {
        /// Requests per hour.
        max_per_hour: u32,
    },
    /// A permit-lifetime bound.
    MaxTtl {
        /// Seconds.
        max_ttl_secs: i64,
    },
}

/// One policy rule as the editor reads and writes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RuleDto {
    /// The rule's stable id.
    pub id: String,
    /// A disabled rule is ignored.
    pub enabled: bool,
    /// A shadow rule is evaluated and recorded, never enforced.
    pub shadow: bool,
    /// A locked rule is an absolute prohibition; only a `deny` can be locked.
    pub locked: bool,
    /// The host dimension.
    pub host: HostScopeDto,
    /// The capability dimension.
    pub capability: PolicyCapabilityDto,
    /// The target dimension.
    pub targets: TargetsDto,
    /// Conditions that decide whether the rule matches.
    #[serde(default)]
    pub conditions: Vec<ConditionDto>,
    /// `allow`, `require_approval` or `deny`.
    pub outcome: String,
    /// Owner text for the editor and the trace.
    #[serde(default)]
    pub note: String,
}

fn effect_text(effect: EffectClass) -> String {
    serde_json::to_value(effect)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn op_text(kind: ConstraintKind) -> &'static str {
    match kind {
        ConstraintKind::IntAtLeast => "int_at_least",
        ConstraintKind::IntAtMost => "int_at_most",
    }
}

fn time_of_day(text: &str) -> Result<TimeOfDay, String> {
    let bad = || format!("{text:?} is not a time of day; write HH:MM");
    let (hour, minute) = text.split_once(':').ok_or_else(bad)?;
    let time = TimeOfDay {
        hour: hour.parse().map_err(|_| bad())?,
        minute: minute.parse().map_err(|_| bad())?,
    };
    if time.is_valid() {
        Ok(time)
    } else {
        Err(bad())
    }
}

impl From<&Rule> for RuleDto {
    fn from(rule: &Rule) -> Self {
        Self {
            id: rule.id.to_string(),
            enabled: rule.enabled,
            shadow: rule.shadow,
            locked: rule.locked,
            host: match &rule.host {
                HostScope::All => HostScopeDto::All,
                HostScope::Host(id) => HostScopeDto::Host {
                    host_id: id.to_string(),
                },
            },
            capability: match &rule.capability {
                CapabilityScope::Any => PolicyCapabilityDto::Any,
                CapabilityScope::Effect(effect) => PolicyCapabilityDto::Effect {
                    effect: effect_text(*effect),
                },
                CapabilityScope::Category(category) => PolicyCapabilityDto::Category {
                    category: category.as_str().to_owned(),
                },
                CapabilityScope::Contract(id) => PolicyCapabilityDto::Contract {
                    contract: id.as_str().to_owned(),
                },
            },
            targets: match &rule.targets {
                TargetScope::Any => TargetsDto::Any,
                TargetScope::Exact(names) => TargetsDto::Exact {
                    names: names.iter().map(|name| name.as_str().to_owned()).collect(),
                },
            },
            conditions: rule.conditions.iter().map(condition_dto).collect(),
            outcome: outcome_name(rule.outcome).to_owned(),
            note: rule.note.clone(),
        }
    }
}

fn outcome_name(outcome: Outcome) -> &'static str {
    outcome.name()
}

fn condition_dto(condition: &Condition) -> ConditionDto {
    match condition {
        Condition::ArgConstraint { arg, op, value } => ConditionDto::ArgConstraint {
            arg: arg.clone(),
            op: op_text(*op).to_owned(),
            value: *value,
        },
        Condition::TimeWindow { start, end } => ConditionDto::TimeWindow {
            start: start.label(),
            end: end.label(),
        },
        Condition::RequiresFact { fact, expected } => ConditionDto::RequiresFact {
            fact: fact.as_str().to_owned(),
            expected: *expected,
        },
        Condition::MaxRate { max_per_hour } => ConditionDto::MaxRate {
            max_per_hour: *max_per_hour,
        },
        Condition::MaxTtl { max_ttl } => ConditionDto::MaxTtl {
            max_ttl_secs: max_ttl.as_secs(),
        },
    }
}

impl TryFrom<RuleDto> for Rule {
    type Error = RuleIssue;

    fn try_from(dto: RuleDto) -> Result<Self, RuleIssue> {
        let id = RuleId::parse(&dto.id).map_err(|err| RuleIssue {
            rule: None,
            message: format!("rule id {:?}: {err}", dto.id),
        })?;
        let issue = |message: String| RuleIssue {
            rule: Some(id.to_string()),
            message,
        };
        let host = match &dto.host {
            HostScopeDto::All => HostScope::All,
            HostScopeDto::Host { host_id } => HostScope::Host(
                host_id
                    .parse::<HostId>()
                    .map_err(|_| issue(format!("{host_id:?} is not a host id")))?,
            ),
        };
        let capability = match &dto.capability {
            PolicyCapabilityDto::Any => CapabilityScope::Any,
            PolicyCapabilityDto::Effect { effect } => CapabilityScope::Effect(
                serde_json::from_value(serde_json::Value::String(effect.clone()))
                    .map_err(|_| issue(format!("{effect:?} is not an effect class")))?,
            ),
            PolicyCapabilityDto::Category { category } => CapabilityScope::Category(
                ContractCategory::parse(category).map_err(|err| issue(err.to_string()))?,
            ),
            PolicyCapabilityDto::Contract { contract } => CapabilityScope::Contract(
                ContractId::parse(contract).map_err(|err| issue(err.to_string()))?,
            ),
        };
        let targets = match &dto.targets {
            TargetsDto::Any => TargetScope::Any,
            TargetsDto::Exact { names } => TargetScope::Exact(
                names
                    .iter()
                    .map(|name| TargetName::parse(name).map_err(|err| issue(err.to_string())))
                    .collect::<Result<BTreeSet<_>, _>>()?,
            ),
        };
        let conditions = dto
            .conditions
            .iter()
            .map(|condition| condition_of(condition).map_err(&issue))
            .collect::<Result<Vec<_>, _>>()?;
        let outcome = match dto.outcome.as_str() {
            "allow" => Outcome::Allow,
            "require_approval" => Outcome::RequireApproval,
            "deny" => Outcome::Deny,
            other => {
                return Err(issue(format!(
                    "{other:?} is not an outcome a rule can have; use allow, require_approval or deny"
                )));
            }
        };
        Ok(Rule {
            id: id.clone(),
            enabled: dto.enabled,
            shadow: dto.shadow,
            locked: dto.locked,
            host,
            capability,
            targets,
            conditions,
            outcome,
            note: dto.note,
            version_id: None,
        })
    }
}

fn condition_of(condition: &ConditionDto) -> Result<Condition, String> {
    Ok(match condition {
        ConditionDto::ArgConstraint { arg, op, value } => Condition::ArgConstraint {
            arg: arg.clone(),
            op: match op.as_str() {
                "int_at_least" => ConstraintKind::IntAtLeast,
                "int_at_most" => ConstraintKind::IntAtMost,
                other => return Err(format!("{other:?} is not int_at_least or int_at_most")),
            },
            value: *value,
        },
        ConditionDto::TimeWindow { start, end } => Condition::TimeWindow {
            start: time_of_day(start)?,
            end: time_of_day(end)?,
        },
        ConditionDto::RequiresFact { fact, expected } => Condition::RequiresFact {
            fact: FactId::parse(fact).map_err(|err| err.to_string())?,
            expected: *expected,
        },
        ConditionDto::MaxRate { max_per_hour } => Condition::MaxRate {
            max_per_hour: *max_per_hour,
        },
        ConditionDto::MaxTtl { max_ttl_secs } => Condition::MaxTtl {
            max_ttl: SignedDuration::from_secs(*max_ttl_secs),
        },
    })
}

/// Converts the editor's rules, collecting every problem.
///
/// # Errors
/// The issues of every rule that does not convert.
pub fn rules_of(dtos: Vec<RuleDto>) -> Result<Vec<Rule>, Vec<RuleIssue>> {
    let mut rules = Vec::with_capacity(dtos.len());
    let mut issues = Vec::new();
    for dto in dtos {
        match Rule::try_from(dto) {
            Ok(rule) => rules.push(rule),
            Err(issue) => issues.push(issue),
        }
    }
    if issues.is_empty() {
        Ok(rules)
    } else {
        Err(issues)
    }
}
