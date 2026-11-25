//! The contracts of `firewall.block_source` and `firewall.list_blocks`.
//!
//! One block is three executions of the same contract (`apply`, `confirm`, `rollback`), all
//! with the same six arguments and the same plan hash. The root helper does the work.

use jiff::SignedDuration;
use serde_json::{Map, Value, json};

use super::contract::{CapabilityContract, ConstrainableArg, ConstraintKind, ContractSource};
use super::effect::{
    EffectClass, ExecutionPath, Idempotency, Reversibility, TargetKind, VerificationHint,
};
use super::ids::{ContractCategory, ContractId, ContractVersion};
use crate::containment::keys;

/// The id of the containment contract.
pub const BLOCK_SOURCE_ID: &str = "firewall.block_source";
/// The id of the read contract.
pub const LIST_BLOCKS_ID: &str = "firewall.list_blocks";

fn block_schema() -> Value {
    let props: Map<String, Value> = keys::ARG_KEYS
        .iter()
        .map(|key| {
            let spec = if *key == keys::ARG_PHASE {
                json!({ "type": "string", "enum": [
                    keys::PHASE_APPLY, keys::PHASE_CONFIRM, keys::PHASE_ROLLBACK
                ] })
            } else if *key == keys::ARG_BLOCK {
                json!({ "type": "string", "format": "uuid" })
            } else {
                json!({ "type": "string" })
            };
            ((*key).to_owned(), spec)
        })
        .collect();
    json!({
        "type": "object",
        "properties": props,
        "required": keys::ARG_KEYS,
        "additionalProperties": false,
    })
}

/// `firewall.block_source` and `firewall.list_blocks`, version 1, in that order.
#[must_use]
pub fn containment_contracts() -> Vec<CapabilityContract> {
    vec![
        CapabilityContract {
            id: ContractId::from_static(BLOCK_SOURCE_ID),
            version: ContractVersion::first(),
            title: "Block source".to_owned(),
            description: "Drops inbound traffic from one external address or small prefix for a limited time, in a dedicated firewall table, through the root helper.".to_owned(),
            input_schema: block_schema(),
            effect: EffectClass::Contain,
            category: ContractCategory::from_static("firewall.contain"),
            target_kind: TargetKind::NetworkSource,
            target_arg: Some(keys::ARG_SOURCE.to_owned()),
            reversibility: Reversibility::Reversible,
            idempotency: Idempotency::Idempotent,
            timeout: SignedDuration::from_secs(60),
            output_limit_bytes: 8192,
            verification: VerificationHint::Postcondition {
                description: "the element is in the block set with time left, the `contain` chain matches its template, and the hub can still reach the agent".to_owned(),
            },
            constrainable_args: vec![
                ConstrainableArg {
                    key: keys::ARG_PREFIX_LEN_V4.to_owned(),
                    kind: ConstraintKind::IntAtLeast,
                    description: "shortest prefix allowed for an IPv4 source".to_owned(),
                },
                ConstrainableArg {
                    key: keys::ARG_PREFIX_LEN_V6.to_owned(),
                    kind: ConstraintKind::IntAtLeast,
                    description: "shortest prefix allowed for an IPv6 source".to_owned(),
                },
            ],
            safety_facts: Vec::new(),
            execution: ExecutionPath::Helper,
            source: ContractSource::BuiltIn,
        },
        CapabilityContract {
            id: ContractId::from_static(LIST_BLOCKS_ID),
            version: ContractVersion::first(),
            title: "List blocks".to_owned(),
            description: "Reads the blocks the root helper holds, the leak self-test gate and the drill state.".to_owned(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false,
            }),
            effect: EffectClass::Read,
            category: ContractCategory::from_static("firewall.observe"),
            target_kind: TargetKind::Host,
            target_arg: None,
            reversibility: Reversibility::NotApplicable,
            idempotency: Idempotency::Idempotent,
            timeout: SignedDuration::from_secs(30),
            output_limit_bytes: 262_144,
            verification: VerificationHint::None,
            constrainable_args: Vec::new(),
            safety_facts: Vec::new(),
            execution: ExecutionPath::Helper,
            source: ContractSource::BuiltIn,
        },
    ]
}
