//! The `AgentLink` behaviour of `FakeAgentLink`.

use super::*;

#[async_trait]
impl AgentLink for FakeAgentLink {
    async fn run_tool(
        &self,
        host: HostId,
        call: ReadToolCall,
        deadline: Timestamp,
    ) -> Result<ToolOutput, LinkError> {
        lock(&self.requests).push(LinkRequest::Tool {
            host,
            call: call.clone(),
            deadline,
        });
        self.connected(host)?;
        let handlers = lock(&self.handlers);
        let handler = handlers.tool.as_ref().ok_or_else(no_handler)?;
        handler(host, &call)
    }

    async fn start_skill(
        &self,
        host: HostId,
        req: SkillDispatch,
        expected_session: AgentSessionId,
        expected_epoch: JournalEpoch,
        deadline: Timestamp,
    ) -> Result<SkillHandoff, LinkError> {
        self.connected(host)?;
        if let Some(error) = lock(&self.handoff_failure).take() {
            return Err(error);
        }
        let gate = lock(&self.handoff_gate).clone();
        if let Some(gate) = gate {
            let _permit = gate
                .acquire_owned()
                .await
                .map_err(|_| LinkError::NotConnected)?;
        }
        if self.session_id(host) != expected_session
            || self.journal_epoch(host) != Some(expected_epoch)
        {
            return Err(LinkError::SessionChanged);
        }
        lock(&self.requests).push(LinkRequest::Skill {
            host,
            dispatch: req.clone(),
            deadline,
        });
        let id = domovoi_core::RequestId::new();
        lock(&self.sent_skills).insert(id, req);
        Ok(SkillHandoff::new(id))
    }

    async fn skill_result(
        &self,
        host: HostId,
        handoff: SkillHandoff,
        _deadline: Timestamp,
    ) -> Result<SkillOutput, LinkError> {
        let Some(req) = lock(&self.sent_skills).remove(&handoff.request()) else {
            return Err(LinkError::Indeterminate(
                "no skill request was handed over".to_owned(),
            ));
        };
        self.connected(host)?;
        let handlers = lock(&self.handlers);
        let handler = handlers.skill.as_ref().ok_or_else(no_handler)?;
        handler(host, &req)
    }

    async fn run_check(
        &self,
        host: HostId,
        check: CheckKey,
        target: Option<TargetKey>,
        deadline: Timestamp,
    ) -> Result<CheckRecheck, LinkError> {
        lock(&self.requests).push(LinkRequest::Check {
            host,
            check: check.clone(),
            target: target.clone(),
            deadline,
        });
        self.connected(host)?;
        let handlers = lock(&self.handlers);
        let handler = handlers.check.as_ref().ok_or_else(no_handler)?;
        handler(host, &check, target.as_ref())
    }

    fn presence(&self, host: HostId) -> Presence {
        lock(&self.presence)
            .get(&host)
            .cloned()
            .unwrap_or(Presence::Disconnected { last_seen: None })
    }

    async fn availability(&self, host: HostId) -> HostAvailability {
        let hold = lock(&self.avail_gates).get(&host).map(|hold| {
            hold.entered.notify_one();
            hold.gate.clone()
        });
        if let Some(gate) = hold {
            // `HostAvailability` has no error channel, so a closed gate leaves the
            // scripted state to answer as it would; tests keep the gate open.
            let _held = gate.acquire_owned().await;
        }
        if let Some(scripted) = lock(&self.availability).get(&host).cloned() {
            return scripted;
        }
        // The trait's default, spelled out because calling it here would recurse.
        match self.presence(host) {
            Presence::Connected { since, .. } => HostAvailability::Known { since },
            Presence::Disconnected {
                last_seen: Some(since),
            } => HostAvailability::Absent { since },
            Presence::Disconnected { last_seen: None } => HostAvailability::Unknown,
        }
    }

    fn journal_epoch(&self, host: HostId) -> Option<JournalEpoch> {
        lock(&self.epochs).get(&host).copied()
    }

    fn snapshot(&self, host: HostId) -> Option<AgentSnapshot> {
        self.snapshot_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if let Some(scripted) = lock(&self.scripted_snapshots).pop_front() {
            return scripted;
        }
        match self.presence(host) {
            Presence::Connected { .. } => Some(AgentSnapshot {
                session: self.session_id(host),
                agent: self.agent_id(host),
                journal_epoch: self.journal_epoch(host).unwrap_or_default(),
                skills: self.skills(host),
            }),
            Presence::Disconnected { .. } => None,
        }
    }

    async fn action_status(
        &self,
        host: HostId,
        ids: Vec<ActionId>,
        expected_epoch: JournalEpoch,
        deadline: Timestamp,
    ) -> Result<ActionAnswers, LinkError> {
        lock(&self.requests).push(LinkRequest::Status {
            host,
            ids: ids.clone(),
            expected_epoch,
            deadline,
        });
        self.connected(host)?;
        let gate = lock(&self.status_gates).get(&host).cloned();
        if let Some(gate) = gate {
            let _permit = gate
                .acquire_owned()
                .await
                .map_err(|_| LinkError::NotConnected)?;
        }
        if let Some(handler) = lock(&self.status).clone() {
            return handler(host, &ids, expected_epoch);
        }
        let scripted = lock(&self.answers);
        let host_answers = scripted.get(&host);
        let answers = ids
            .into_iter()
            .map(|id| ActionAnswerEntry {
                execution: id,
                answer: host_answers
                    .and_then(|known| known.get(&id))
                    .cloned()
                    .unwrap_or(ActionAnswer::NotFound),
            })
            .collect();
        Ok(ActionAnswers {
            epoch: expected_epoch,
            answers,
        })
    }

    fn skills(&self, host: HostId) -> Vec<SkillSummary> {
        lock(&self.skills).get(&host).cloned().unwrap_or_default()
    }

    fn catalog_hash(&self, host: HostId) -> Option<String> {
        self.catalogue.hash(host)
    }

    fn host_view(&self, host: HostId) -> Option<HostView> {
        if !matches!(self.presence(host), Presence::Connected { .. }) {
            return None;
        }
        Some(HostView {
            words: lock(&self.words).get(&host).cloned().unwrap_or_default(),
            skills: self.skills(host),
        })
    }

    async fn send_transfer(
        &self,
        host: HostId,
        frame: TransferFrame,
        expected_session: AgentSessionId,
        deadline: Timestamp,
    ) -> Result<Option<TransferAnswer>, LinkError> {
        self.fake_transfer(host, frame, expected_session, deadline)
            .await
    }

    async fn start_capability(
        &self,
        host: HostId,
        req: PackageDispatch,
        expected_session: AgentSessionId,
        expected_epoch: JournalEpoch,
        deadline: Timestamp,
    ) -> Result<CapabilityHandoff, LinkError> {
        self.fake_start_capability(host, req, expected_session, expected_epoch, deadline)
            .await
    }

    async fn capability_result(
        &self,
        host: HostId,
        handoff: CapabilityHandoff,
        deadline: Timestamp,
    ) -> Result<PackageOutputs, LinkError> {
        self.fake_capability_result(host, handoff, deadline).await
    }

    async fn attest_safety(
        &self,
        host: HostId,
        nonce: domovoi_core::attest::Nonce32,
        _deadline: Timestamp,
    ) -> Result<domovoi_core::ports::AttestationReply, LinkError> {
        self.connected(host)?;
        self.attest.answer(host, nonce)
    }

    fn helper_report(&self, host: HostId) -> Option<domovoi_core::ports::HelperReport> {
        if !matches!(self.presence(host), Presence::Connected { .. }) {
            return None;
        }
        self.attest.report(host)
    }

    async fn get_catalog(
        &self,
        host: HostId,
        query: CatalogQuery,
        deadline: Timestamp,
    ) -> Result<CatalogPageData, LinkError> {
        self.connected(host)?;
        self.catalogue.page(host, &query, deadline)
    }
}
