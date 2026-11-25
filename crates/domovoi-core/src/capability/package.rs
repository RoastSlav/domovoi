//! The three contracts of the `agent.package.*` capabilities.
//!
//! Kept apart from [`builtin_contracts`](super::builtin_contracts), which is the ten read
//! tools; a hub and an agent add these as an extra source.

use jiff::SignedDuration;
use serde_json::{Value, json};

use super::contract::{CapabilityContract, ContractSource};
use super::effect::{
    EffectClass, ExecutionPath, Idempotency, Reversibility, TargetKind, VerificationHint,
};
use super::ids::{ContractCategory, ContractId, ContractVersion};
use crate::package::keys;

/// An object schema whose listed string properties are all required and nothing else is
/// allowed. The flag marks properties that are UUID strings.
fn schema(properties: &[(&str, bool)]) -> Value {
    let props: serde_json::Map<String, Value> = properties
        .iter()
        .map(|(key, uuid)| {
            let spec = if *uuid {
                json!({ "type": "string", "format": "uuid" })
            } else {
                json!({ "type": "string" })
            };
            ((*key).to_owned(), spec)
        })
        .collect();
    let required: Vec<&str> = properties.iter().map(|(key, _)| *key).collect();
    json!({
        "type": "object",
        "properties": props,
        "required": required,
        "additionalProperties": false,
    })
}

/// One mutating package contract.
fn mutating(
    id: &'static str,
    title: &str,
    description: &str,
    input_schema: Value,
    timeout_secs: i64,
    postcondition: &str,
) -> CapabilityContract {
    CapabilityContract {
        id: ContractId::from_static(id),
        version: ContractVersion::first(),
        title: title.to_owned(),
        description: description.to_owned(),
        input_schema,
        effect: EffectClass::Mutate,
        category: ContractCategory::from_static("package.manage"),
        target_kind: TargetKind::Host,
        target_arg: None,
        reversibility: Reversibility::Reversible,
        idempotency: Idempotency::Idempotent,
        timeout: SignedDuration::from_secs(timeout_secs),
        output_limit_bytes: 8192,
        verification: VerificationHint::Postcondition {
            description: postcondition.to_owned(),
        },
        constrainable_args: Vec::new(),
        safety_facts: Vec::new(),
        execution: ExecutionPath::Agent,
        source: ContractSource::BuiltIn,
    }
}

/// Every contract the hub knows by id: the built-in table plus the package contracts, which
/// the built-in table deliberately leaves out.
#[must_use]
pub fn known_contracts() -> Vec<CapabilityContract> {
    super::builtin_contracts()
        .into_iter()
        .chain(package_contracts())
        .chain(super::unit::unit_contracts())
        .chain(super::containment::containment_contracts())
        .collect()
}

/// The contracts of `agent.package.install`, `agent.package.uninstall` and
/// `agent.package.list`, version 1, in that order.
#[must_use]
pub fn package_contracts() -> Vec<CapabilityContract> {
    vec![
        mutating(
            "agent.package.install",
            "Install package",
            "Verifies a signed package against the keys on the host and installs it without enabling it.",
            schema(&[
                (keys::ARG_PACKAGE, false),
                (keys::ARG_VERSION, false),
                (keys::ARG_ARTIFACT_TRANSFER, true),
                (keys::ARG_SIGNATURE_TRANSFER, true),
            ]),
            300,
            "the package appears in agent.package.list with the signed key id and archive digest",
        ),
        mutating(
            "agent.package.uninstall",
            "Uninstall package",
            "Removes an installed package and its files from the host.",
            schema(&[(keys::ARG_PACKAGE, false), (keys::ARG_VERSION, false)]),
            60,
            "the package no longer appears in agent.package.list",
        ),
        CapabilityContract {
            id: ContractId::from_static("agent.package.list"),
            version: ContractVersion::first(),
            title: "List packages".to_owned(),
            description: "Lists the packages installed on the host with their signer and status."
                .to_owned(),
            input_schema: schema(&[]),
            effect: EffectClass::Read,
            category: ContractCategory::from_static("package.observe"),
            target_kind: TargetKind::Host,
            target_arg: None,
            reversibility: Reversibility::NotApplicable,
            idempotency: Idempotency::Idempotent,
            timeout: SignedDuration::from_secs(30),
            output_limit_bytes: 262_144,
            verification: VerificationHint::None,
            constrainable_args: Vec::new(),
            safety_facts: Vec::new(),
            execution: ExecutionPath::Agent,
            source: ContractSource::BuiltIn,
        },
    ]
}
