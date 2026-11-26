//! Tests of the default methods of the agent link.

use super::*;

/// A link that only knows presence, so it exercises the default `availability`.
struct PresenceOnly(Presence);

#[async_trait]
impl AgentLink for PresenceOnly {
    async fn run_tool(
        &self,
        _host: HostId,
        _call: ReadToolCall,
        _deadline: Timestamp,
    ) -> Result<ToolOutput, LinkError> {
        Err(LinkError::NotConnected)
    }

    async fn start_skill(
        &self,
        _host: HostId,
        _req: SkillDispatch,
        _expected_session: crate::id::AgentSessionId,
        _expected_epoch: JournalEpoch,
        _deadline: Timestamp,
    ) -> Result<SkillHandoff, LinkError> {
        Err(LinkError::NotConnected)
    }

    fn snapshot(&self, _host: HostId) -> Option<AgentSnapshot> {
        None
    }

    async fn skill_result(
        &self,
        _host: HostId,
        _handoff: SkillHandoff,
        _deadline: Timestamp,
    ) -> Result<SkillOutput, LinkError> {
        Err(LinkError::NotConnected)
    }

    async fn run_check(
        &self,
        _host: HostId,
        _check: CheckKey,
        _target: Option<TargetKey>,
        _deadline: Timestamp,
    ) -> Result<CheckRecheck, LinkError> {
        Err(LinkError::NotConnected)
    }

    fn presence(&self, _host: HostId) -> Presence {
        self.0.clone()
    }

    fn journal_epoch(&self, _host: HostId) -> Option<JournalEpoch> {
        None
    }

    async fn action_status(
        &self,
        _host: HostId,
        _ids: Vec<ExecutionId>,
        _expected_epoch: JournalEpoch,
        _deadline: Timestamp,
    ) -> Result<ActionAnswers, LinkError> {
        Err(LinkError::NotConnected)
    }

    fn skills(&self, _host: HostId) -> Vec<SkillSummary> {
        Vec::new()
    }
}

#[test]
fn availability_default_follows_presence() {
    futures::executor::block_on(async {
        let host = HostId::new();
        let since = Timestamp::constant(1_700_000_000, 0);

        let connected = PresenceOnly(Presence::Connected {
            since,
            version: "0.1.0".to_owned(),
        });
        assert_eq!(
            connected.availability(host).await,
            HostAvailability::Known { since },
        );

        let left = PresenceOnly(Presence::Disconnected {
            last_seen: Some(since),
        });
        assert_eq!(
            left.availability(host).await,
            HostAvailability::Absent { since },
        );

        let never = PresenceOnly(Presence::Disconnected { last_seen: None });
        assert_eq!(never.availability(host).await, HostAvailability::Unknown);
    });
}

#[test]
fn package_operations_default_to_not_supported() {
    use crate::install::{CapabilityHandoff, PackageCall, PackageDispatch, TransferFrame};

    futures::executor::block_on(async {
        let link = PresenceOnly(Presence::Disconnected { last_seen: None });
        let host = HostId::new();
        let session = crate::id::AgentSessionId::new();
        let now = Timestamp::constant(1_700_000_000, 0);
        let unsupported = LinkError::Transport("not supported by this link".to_owned());
        let frame = TransferFrame::End {
            transfer: uuid::Uuid::from_u128(1),
        };
        assert_eq!(
            link.send_transfer(host, frame, session, now).await,
            Err(unsupported.clone())
        );
        let dispatch = PackageDispatch {
            execution: crate::id::ExecutionId::new(),
            call: PackageCall::List,
            plan_hash: "00".repeat(32).parse().unwrap(),
            expires_at: now,
        };
        assert_eq!(
            link.start_capability(host, dispatch, session, JournalEpoch::default(), now)
                .await,
            Err(unsupported.clone())
        );
        let handoff = CapabilityHandoff::new(RequestId::new());
        assert_eq!(
            link.capability_result(host, handoff, now).await,
            Err(unsupported)
        );
    });
}
