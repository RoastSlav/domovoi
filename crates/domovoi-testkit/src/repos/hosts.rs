use async_trait::async_trait;
use domovoi_core::ports::{EnrollmentToken, HostRepo, StoreError};
use domovoi_core::{AgentId, AgentIdentity, Host, HostId, HostName};
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

    async fn revoke_agent(&self, id: AgentId, at: Timestamp) -> Result<(), StoreError> {
        let mut state = self.begin()?;
        let agent = state.agents.get_mut(&id).ok_or(StoreError::NotFound)?;
        agent.revoked_at.get_or_insert(at);
        Ok(())
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
