//! The contracts of `unit.identity`, `unit.restart` and `container.restart`.
//!
//! `unit.restart` is the only contract a policy permit may dispatch. `container.restart`
//! exists so rules can name it and the registry can explain it; nothing dispatches it.

use jiff::SignedDuration;
use serde_json::{Value, json};

use super::contract::{CapabilityContract, ContractSource};
use super::effect::{
    EffectClass, ExecutionPath, Idempotency, Reversibility, TargetKind, VerificationHint,
};
use super::ids::{ContractCategory, ContractId, ContractVersion};

/// The contracts a policy permit may dispatch. A closed list: `firewall.block_source` is the
/// second; nothing else is added here.
pub const POLICY_DISPATCHABLE: &[&str] = &["unit.restart", "firewall.block_source"];

/// Whether a policy permit may dispatch `id`.
#[must_use]
pub fn is_policy_dispatchable(id: &ContractId) -> bool {
    POLICY_DISPATCHABLE.contains(&id.as_str())
}

/// The policy-dispatchable contracts a person can never run by hand: containment is driven
/// by policy only, so the host's unattended limit is what makes it available at all.
pub const POLICY_ONLY: &[&str] = &["firewall.block_source"];

/// Whether `id` runs only through policy, so a missing unattended limit refuses it.
#[must_use]
pub fn is_policy_only(id: &ContractId) -> bool {
    POLICY_ONLY.contains(&id.as_str())
}

/// The built-in contracts a plan may execute, besides skills.
pub const EXECUTABLE_BUILTINS: &[&str] = &["unit.restart", "firewall.block_source"];

fn schema(property: &str) -> Value {
    json!({
        "type": "object",
        "properties": { property: { "type": "string" } },
        "required": [property],
        "additionalProperties": false,
    })
}

/// The fields that differ between the three contracts.
struct Spec {
    id: &'static str,
    title: &'static str,
    description: &'static str,
    effect: EffectClass,
    category: &'static str,
    target_kind: TargetKind,
    arg: &'static str,
    reversibility: Reversibility,
    timeout_secs: i64,
    output_limit_bytes: u32,
    verification: VerificationHint,
}

fn contract(spec: Spec) -> CapabilityContract {
    CapabilityContract {
        id: ContractId::from_static(spec.id),
        version: ContractVersion::first(),
        title: spec.title.to_owned(),
        description: spec.description.to_owned(),
        input_schema: schema(spec.arg),
        effect: spec.effect,
        category: ContractCategory::from_static(spec.category),
        target_kind: spec.target_kind,
        target_arg: Some(spec.arg.to_owned()),
        reversibility: spec.reversibility,
        idempotency: Idempotency::Idempotent,
        timeout: SignedDuration::from_secs(spec.timeout_secs),
        output_limit_bytes: spec.output_limit_bytes,
        verification: spec.verification,
        constrainable_args: Vec::new(),
        safety_facts: Vec::new(),
        execution: ExecutionPath::Helper,
        source: ContractSource::BuiltIn,
    }
}

/// `unit.identity`, `unit.restart` and `container.restart`, version 1, in that order.
#[must_use]
pub fn unit_contracts() -> Vec<CapabilityContract> {
    vec![
        contract(Spec {
            id: "unit.identity",
            title: "Read unit identity",
            description: "Reads the loaded configuration and files of one systemd service.",
            effect: EffectClass::Read,
            category: "unit.observe",
            target_kind: TargetKind::Unit,
            arg: "unit",
            reversibility: Reversibility::NotApplicable,
            timeout_secs: 30,
            output_limit_bytes: 16 * 1024,
            verification: VerificationHint::None,
        }),
        contract(Spec {
            id: "unit.restart",
            title: "Restart service",
            description: "Restarts one systemd service the owner allowed, after checking that it still runs what was approved.",
            effect: EffectClass::Mutate,
            category: "unit.lifecycle",
            target_kind: TargetKind::Unit,
            arg: "unit",
            reversibility: Reversibility::Reversible,
            timeout_secs: 60,
            output_limit_bytes: 8 * 1024,
            verification: VerificationHint::Postcondition {
                description: "the new process runs the configuration you approved".to_owned(),
            },
        }),
        contract(Spec {
            id: "container.restart",
            title: "Restart container",
            description: "Restarts one container. Named by rules only; nothing dispatches it yet.",
            effect: EffectClass::Mutate,
            category: "container.lifecycle",
            target_kind: TargetKind::Container,
            arg: "container",
            reversibility: Reversibility::Reversible,
            timeout_secs: 60,
            output_limit_bytes: 8 * 1024,
            verification: VerificationHint::Postcondition {
                description: "the container is running again".to_owned(),
            },
        }),
    ]
}
