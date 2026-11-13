use async_trait::async_trait;
use domovoi_core::ports::{EnrollmentToken, HostRepo, StoreError};
use domovoi_core::{
    AgentId, AgentIdentity, AgentReport, AuthenticatedReport, Host, HostId, HostName,
    REPORT_REFRESH, ReportOutcome,
};
use jiff::Timestamp;

use super::{InMemoryStore, conflict};

#[async_trait]
impl HostRepo for InMemoryStore {
    async fn upsert_host(&self, host: &Host) -> Result<(), StoreError> {
        let mut state = self.begin()?;
        let taken = state
            .hosts
            .values()
            .any(|h| h.name == host.name && h.id != host.id);
        if taken {
            return Err(conflict("host name taken"));
        }
        state.hosts.insert(host.id, host.clone());
        Ok(())
    }

    async fn get_host(&self, id: HostId) -> Result<Option<Host>, StoreError> {
        Ok(self.begin()?.hosts.get(&id).cloned())
    }

    async fn get_host_by_name(&self, name: &HostName) -> Result<Option<Host>, StoreError> {
        Ok(self
            .begin()?
            .hosts
            .values()
            .find(|h| &h.name == name)
            .cloned())
    }

    async fn list_hosts(&self) -> Result<Vec<Host>, StoreError> {
        let mut hosts: Vec<Host> = self.begin()?.hosts.values().cloned().collect();
        hosts.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(hosts)
    }

    async fn put_agent(&self, agent: &AgentIdentity) -> Result<(), StoreError> {
        let mut state = self.begin()?;
        if !state.hosts.contains_key(&agent.host) {
            return Err(StoreError::NotFound);
        }
        if let Some(existing) = state.agents.get(&agent.agent)
            && existing.revoked_at.is_some()
        {
            return Err(conflict("agent revoked"));
        }
        let other_active = state
            .agents
            .values()
            .any(|a| a.host == agent.host && a.agent != agent.agent && a.revoked_at.is_none());
        if other_active {
            return Err(conflict("host already has an agent"));
        }
        state.agents.insert(agent.agent, agent.clone());
        Ok(())
    }

    async fn get_agent(&self, id: AgentId) -> Result<Option<AgentIdentity>, StoreError> {
        Ok(self.begin()?.agents.get(&id).cloned())
    }

    async fn live_agent(&self, host: HostId) -> Result<Option<AgentIdentity>, StoreError> {
        let state = self.begin()?;
        Ok(state
            .agents
            .values()
            .find(|a| a.host == host && a.revoked_at.is_none())
            .cloned())
    }

    async fn revoke_agent(&self, id: AgentId, at: Timestamp) -> Result<(), StoreError> {
        let mut state = self.begin()?;
        let agent = state.agents.get_mut(&id).ok_or(StoreError::NotFound)?;
        agent.revoked_at.get_or_insert(at);
        Ok(())
    }

    async fn agent_report(&self, agent: AgentId) -> Result<Option<AgentReport>, StoreError> {
        Ok(self.begin()?.reports.get(&agent).cloned())
    }

    async fn record_agent_report(
        &self,
        agent: AgentId,
        seen: &AuthenticatedReport,
        now: Timestamp,
    ) -> Result<ReportOutcome, StoreError> {
        let mut state = self.begin()?;
        if !state.agents.contains_key(&agent) {
            return Err(StoreError::NotFound);
        }
        let previous = state.reports.get(&agent).cloned();
        let since_was = previous
            .as_ref()
            .and_then(|report| report.incompatible_since);
        let changed = previous.as_ref().is_none_or(|report| {
            report.protocol != seen.protocol
                || report.release != seen.release
                || report.incompatible_since.is_some() != !seen.compatible
        });
        if changed {
            let since = if seen.compatible {
                None
            } else {
                since_was.or(Some(now))
            };
            state.reports.insert(
                agent,
                AgentReport {
                    protocol: seen.protocol,
                    release: seen.release.clone(),
                    last_authenticated_at: now,
                    incompatible_since: since,
                },
            );
        } else if previous
            .as_ref()
            .is_none_or(|report| now.duration_since(report.last_authenticated_at) >= REPORT_REFRESH)
            && let Some(report) = state.reports.get_mut(&agent)
        {
            report.last_authenticated_at = now;
        }
        Ok(ReportOutcome { changed, previous })
    }

    async fn create_enrollment_token(&self, token: &EnrollmentToken) -> Result<(), StoreError> {
        let mut state = self.begin()?;
        if !state.hosts.contains_key(&token.host) {
            return Err(StoreError::NotFound);
        }
        if state.tokens.contains_key(&token.token_hash) {
            return Err(conflict("enrollment token exists"));
        }
        state.tokens.insert(token.token_hash, token.clone());
        Ok(())
    }

    async fn consume_enrollment_token(
        &self,
        token_hash: &[u8; 32],
        now: Timestamp,
    ) -> Result<HostId, StoreError> {
        let mut state = self.begin()?;
        let token = state
            .tokens
            .remove(token_hash)
            .ok_or(StoreError::NotFound)?;
        if now >= token.expires_at {
            return Err(StoreError::NotFound);
        }
        Ok(token.host)
    }
}
