//! The built-in contracts derived from the closed `ReadTool` catalogue.
//!
//! Each non-skill read tool gets exactly one contract. The model still calls the tool by
//! its snake_case name; the contract id is the registry and policy name for the same
//! capability, and the arguments come from the tool's own JSON Schema so the two cannot
//! drift.

use jiff::SignedDuration;

use super::contract::{CapabilityContract, ContractSource};
use super::effect::{
    EffectClass, ExecutionPath, Idempotency, Reversibility, TargetKind, VerificationHint,
};
use super::ids::{ContractCategory, ContractId, ContractVersion};
use crate::triage::ReadTool;

/// How long one built-in read may take.
///
/// Equals `domovoi_engine::triage::tools::TOOL_DEADLINE`; an engine test pins the equality.
pub const READ_TIMEOUT: SignedDuration = SignedDuration::from_secs(15);

/// The most output the hub keeps from one built-in read, in bytes.
///
/// Equals `domovoi_engine::triage::tools::MAX_RESULT_BYTES`; an engine test pins the equality.
pub const READ_OUTPUT_LIMIT: u32 = 8 * 1024;

/// The contracts of the ten non-skill read tools, in [`ReadTool::ALL`] order, version 1.
#[must_use]
pub fn builtin_contracts() -> Vec<CapabilityContract> {
    ReadTool::ALL
        .iter()
        .filter_map(|tool| contract_for(*tool))
        .collect()
}

/// The built-in contract id a read tool is the model-facing name of; `None` for
/// [`ReadTool::RunReadSkill`], which is the entry point to the skill family.
#[must_use]
pub fn contract_id_of(tool: ReadTool) -> Option<ContractId> {
    let id = match tool {
        ReadTool::DockerPs => "container.list",
        ReadTool::DockerLogs => "container.logs",
        ReadTool::DockerInspect => "container.inspect",
        ReadTool::DockerEvents => "container.events",
        ReadTool::HostOverview => "host.overview",
        ReadTool::Journal => "system.journal",
        ReadTool::ListeningPorts => "network.listening_ports",
        ReadTool::SmartReport => "disk.smart_report",
        ReadTool::IncidentHistory => "incident.history",
        ReadTool::NotesSearch => "notes.search",
        ReadTool::RunReadSkill => return None,
    };
    Some(ContractId::from_static(id))
}

/// One built-in read contract, or `None` for the skill entry point.
fn contract_for(tool: ReadTool) -> Option<CapabilityContract> {
    let (id, category, target_kind, target_arg, execution) = match tool {
        ReadTool::DockerPs => (
            "container.list",
            "container.observe",
            TargetKind::Host,
            None,
            ExecutionPath::Agent,
        ),
        ReadTool::DockerLogs => (
            "container.logs",
            "container.observe",
            TargetKind::Container,
            Some("container"),
            ExecutionPath::Agent,
        ),
        ReadTool::DockerInspect => (
            "container.inspect",
            "container.observe",
            TargetKind::Container,
            Some("container"),
            ExecutionPath::Agent,
        ),
        ReadTool::DockerEvents => (
            "container.events",
            "container.observe",
            TargetKind::Host,
            None,
            ExecutionPath::Agent,
        ),
        ReadTool::HostOverview => (
            "host.overview",
            "host.observe",
            TargetKind::Host,
            None,
            ExecutionPath::Agent,
        ),
        ReadTool::Journal => (
            "system.journal",
            "host.observe",
            TargetKind::Host,
            None,
            ExecutionPath::Agent,
        ),
        ReadTool::ListeningPorts => (
            "network.listening_ports",
            "network.observe",
            TargetKind::Host,
            None,
            ExecutionPath::Agent,
        ),
        ReadTool::SmartReport => (
            "disk.smart_report",
            "disk.observe",
            TargetKind::Host,
            None,
            ExecutionPath::Agent,
        ),
        ReadTool::IncidentHistory => (
            "incident.history",
            "knowledge.observe",
            TargetKind::Host,
            None,
            ExecutionPath::Hub,
        ),
        ReadTool::NotesSearch => (
            "notes.search",
            "knowledge.observe",
            TargetKind::Host,
            None,
            ExecutionPath::Hub,
        ),
        ReadTool::RunReadSkill => return None,
    };

    Some(CapabilityContract {
        id: ContractId::from_static(id),
        version: ContractVersion::first(),
        title: title_of(tool),
        description: tool.description().to_owned(),
        input_schema: tool.json_schema(),
        effect: EffectClass::Read,
        category: ContractCategory::from_static(category),
        target_kind,
        target_arg: target_arg.map(str::to_owned),
        reversibility: Reversibility::NotApplicable,
        idempotency: Idempotency::Idempotent,
        timeout: READ_TIMEOUT,
        output_limit_bytes: READ_OUTPUT_LIMIT,
        verification: VerificationHint::None,
        constrainable_args: Vec::new(),
        safety_facts: Vec::new(),
        execution,
        containment: None,
        source: ContractSource::BuiltIn,
    })
}

/// The tool's name with underscores turned into spaces and its first letter upper-cased.
fn title_of(tool: ReadTool) -> String {
    let name = tool.name().replace('_', " ");
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => name,
    }
}
