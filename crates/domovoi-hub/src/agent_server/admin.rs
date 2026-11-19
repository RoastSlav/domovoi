//! Agent administration the transports need: enrollment, revocation, presence and
//! catalogues, as a port so a service or a test can stand in for the agent server.

use async_trait::async_trait;
use domovoi_core::ports::{LinkError, LogLine, Presence};
use domovoi_core::{HostId, HostName, SkillSummary};
use domovoi_proto::messages::{CheckInfo, HostFacts};

use super::{AgentServer, AgentServerError, EnrollmentInvite};

/// Agent administration the hub needs: enrollment, revocation, presence and catalogues.
#[async_trait]
pub trait AgentAdmin: Send + Sync {
    /// A one-time enrollment invite for host `name`, creating the host if needed.
    async fn create_invite(&self, name: &HostName) -> Result<EnrollmentInvite, AgentServerError>;
    /// Revokes the live agent of `host`; `false` when it had none.
    async fn revoke(&self, host: HostId) -> Result<bool, AgentServerError>;
    /// Whether `host`'s agent is connected.
    fn presence(&self, host: HostId) -> Presence;
    /// What `host`'s agent said about its machine when it connected; `None` when it never
    /// completed a handshake.
    fn facts(&self, host: HostId) -> Option<HostFacts>;
    /// The checks `host`'s agent left on, as its own config set them.
    fn checks(&self, host: HostId) -> Vec<CheckInfo>;
    /// The skills `host`'s agent reported.
    fn skills(&self, host: HostId) -> Vec<SkillSummary>;
    /// The recent log lines `host`'s agent keeps, oldest first. Fails with a
    /// [`domovoi_core::ports::LinkError`] when it is not connected, did not answer,
    /// or is too old to send logs.
    async fn logs(
        &self,
        host: HostId,
        lines: u16,
        min_level: String,
    ) -> Result<Vec<LogLine>, LinkError>;
}

#[async_trait]
impl AgentAdmin for AgentServer {
    async fn create_invite(&self, name: &HostName) -> Result<EnrollmentInvite, AgentServerError> {
        self.create_token(name).await
    }

    async fn revoke(&self, host: HostId) -> Result<bool, AgentServerError> {
        self.revoke_agent(host).await
    }

    fn presence(&self, host: HostId) -> Presence {
        self.host_presence(host)
    }

    fn facts(&self, host: HostId) -> Option<HostFacts> {
        self.host_facts(host)
    }

    fn checks(&self, host: HostId) -> Vec<CheckInfo> {
        self.host_checks(host)
    }

    fn skills(&self, host: HostId) -> Vec<SkillSummary> {
        self.host_skills(host)
    }

    async fn logs(
        &self,
        host: HostId,
        lines: u16,
        min_level: String,
    ) -> Result<Vec<LogLine>, LinkError> {
        self.host_logs(host, lines, min_level).await
    }
}
