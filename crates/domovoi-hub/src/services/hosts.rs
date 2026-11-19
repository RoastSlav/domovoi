//! A host's status as every transport shows it: presence, protocol state and open incidents.

use std::sync::Arc;

use domovoi_core::ports::{HostRepo, IncidentFilter, IncidentRepo, Page, Presence, StoreError};
use domovoi_core::{AgentReport, Host, HostId, HostKind, HostName};
use jiff::Timestamp;

use crate::agent_server::{AgentAdmin, ProtocolState, protocol_state};
use crate::version::{HUB_VERSION, is_older};

/// The most unresolved incidents counted per host.
pub const OPEN_COUNT_CAP: u16 = 200;

/// One host as a person sees it: what the hub knows of its agent right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostStatus {
    /// The host.
    pub id: HostId,
    /// Its name.
    pub name: HostName,
    /// Where it runs relative to the hub.
    pub kind: HostKind,
    /// Whether its agent is connected.
    pub connected: bool,
    /// Whether an agent is enrolled for it.
    pub agent_enrolled: bool,
    /// The connected agent's version.
    pub version: Option<String>,
    /// When the agent connected.
    pub connected_since: Option<Timestamp>,
    /// When it was last seen, while disconnected.
    pub last_seen: Option<Timestamp>,
    /// Whether the connected agent is older than the hub.
    pub outdated: bool,
    /// The wire name of the protocol state.
    pub protocol_state: &'static str,
    /// The protocol the agent reported.
    pub reported_protocol: Option<String>,
    /// The release the agent reported.
    pub reported_release: Option<String>,
    /// Since when the agent has been incompatible.
    pub incompatible_since: Option<Timestamp>,
    /// The protocol this hub speaks.
    pub hub_protocol: String,
    /// Unresolved incidents, capped at [`OPEN_COUNT_CAP`].
    pub open_incidents: u32,
}

/// Builds host statuses.
#[derive(Clone)]
pub struct HostService {
    pub(super) incidents: Arc<dyn IncidentRepo>,
    pub(super) hosts: Arc<dyn HostRepo>,
    pub(super) agents: Arc<dyn AgentAdmin>,
}

impl HostService {
    /// The status of `host`, with its agent's presence and its count of unresolved incidents.
    pub async fn status(&self, host: &Host) -> Result<HostStatus, StoreError> {
        let filter = IncidentFilter {
            host: Some(host.id),
            active: Some(true),
            ..IncidentFilter::default()
        };
        let page = Page {
            cursor: None,
            limit: OPEN_COUNT_CAP,
        };
        let open = self.incidents.list(&filter, &page).await?.items.len();
        let presence = self.agents.presence(host.id);
        let connected = matches!(presence, Presence::Connected { .. });
        let agent = self.hosts.live_agent(host.id).await?;
        let report = match &agent {
            Some(agent) => self.hosts.agent_report(agent.agent).await?,
            None => None,
        };
        let state_name = if connected {
            "compatible"
        } else {
            protocol_name(&report)
        };
        let mut status = HostStatus {
            id: host.id,
            name: host.name.clone(),
            kind: host.kind,
            connected,
            agent_enrolled: agent.is_some(),
            version: None,
            connected_since: None,
            last_seen: None,
            outdated: false,
            protocol_state: state_name,
            reported_protocol: report.as_ref().map(|report| report.protocol.to_string()),
            reported_release: report.as_ref().map(|report| report.release.clone()),
            incompatible_since: report.as_ref().and_then(|report| report.incompatible_since),
            hub_protocol: domovoi_proto::PROTOCOL_VERSION.to_string(),
            open_incidents: u32::try_from(open).unwrap_or(u32::MAX),
        };
        match presence {
            Presence::Connected { since, version } => {
                status.outdated = is_older(&version, HUB_VERSION);
                status.version = Some(version);
                status.connected_since = Some(since);
            }
            Presence::Disconnected { last_seen } => status.last_seen = last_seen,
        }
        Ok(status)
    }
}

/// The wire name of a host's protocol state.
fn protocol_name(report: &Option<AgentReport>) -> &'static str {
    match protocol_state(report.as_ref(), domovoi_proto::PROTOCOL_VERSION) {
        ProtocolState::Unknown => "unknown",
        ProtocolState::Compatible => "compatible",
        ProtocolState::Incompatible { .. } => "incompatible",
        ProtocolState::ExpectedIncompatible { .. } => "expected_incompatible",
    }
}
