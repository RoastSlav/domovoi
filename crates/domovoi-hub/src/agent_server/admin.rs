//! Agent administration the transports need: enrollment, revocation, presence and
//! catalogues, as a port so a service or a test can stand in for the agent server.

use async_trait::async_trait;
use domovoi_core::ports::{LinkError, LogLine, Presence};
use domovoi_core::{HostId, HostName, SkillSummary};
use domovoi_proto::messages::{CheckInfo, HostFacts};
use std::collections::BTreeMap;

use super::{AgentServer, AgentServerError, EnrollmentInvite, PackageReport};

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
    /// What identifies `host`'s live agent and what it can do now, each under a name; a
    /// changed value means a stored host profile describes another state. Empty while
    /// offline.
    async fn runtime_basis(&self, _host: HostId) -> BTreeMap<String, String> {
        BTreeMap::new()
    }
    /// The checks `host`'s agent left on, as its own config set them.
    fn checks(&self, host: HostId) -> Vec<CheckInfo>;
    /// The skills `host`'s agent reported.
    fn skills(&self, host: HostId) -> Vec<SkillSummary>;
    /// The skills of the packages stored on `host`, on or off: as its live agent announced
    /// them, else as it announced them when last connected.
    async fn package_report(&self, _host: HostId) -> PackageReport {
        PackageReport::default()
    }
    /// Notes the agent's answer to an enable or disable of a package's skills.
    async fn note_package_enabled(
        &self,
        _host: HostId,
        _package: &str,
        _version: &str,
        _enabled: bool,
    ) {
    }
    /// Whether `host`'s live agent reports that it runs in a container.
    fn in_container(&self, _host: HostId) -> bool {
        false
    }
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

    async fn package_report(&self, host: HostId) -> PackageReport {
        self.host_package_report(host).await
    }

    async fn note_package_enabled(
        &self,
        host: HostId,
        package: &str,
        version: &str,
        enabled: bool,
    ) {
        AgentServer::note_package_enabled(self, host, package, version, enabled).await;
    }

    async fn runtime_basis(&self, host: HostId) -> BTreeMap<String, String> {
        self.host_basis(host).await
    }

    fn in_container(&self, host: HostId) -> bool {
        self.host_in_container(host)
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
