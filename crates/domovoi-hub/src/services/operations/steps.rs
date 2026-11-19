//! Approving and denying an Execution step, from the web or from a chat button.

use domovoi_core::{
    ExecutionRecord, OperationId, OperationSnapshot, PlanBinding, Risk, SkillName, SkillSummary,
    StepId, StepKind,
};

use super::{OperationCommandError, OperationCommands};
use crate::auth::approval::{approval_for, authorization_for};
use crate::services::{Caller, Surface};

/// The risk of `skill` on a host's current skill list; `None` (unknown) when it is not there.
#[must_use]
pub fn risk_of_skill(skills: &[SkillSummary], skill: &SkillName) -> Option<Risk> {
    skills
        .iter()
        .find(|summary| &summary.name == skill)
        .map(|summary| summary.risk)
}

fn store_error(e: domovoi_core::ports::StoreError) -> OperationCommandError {
    OperationCommandError::Operation(domovoi_engine::operations::OperationError::Store(
        e.to_string(),
    ))
}

impl OperationCommands {
    async fn snapshot(&self, op: OperationId) -> Result<OperationSnapshot, OperationCommandError> {
        self.repo
            .load(op)
            .await
            .map_err(store_error)?
            .ok_or(OperationCommandError::NotFound)
    }

    /// The Execution step's record, or `NotFound` when `step` is not one of this operation.
    fn execution(
        snapshot: &OperationSnapshot,
        step: StepId,
    ) -> Result<&ExecutionRecord, OperationCommandError> {
        snapshot
            .step(step)
            .filter(|record| record.step.kind == StepKind::Execution)
            .ok_or(OperationCommandError::NotFound)?;
        snapshot
            .execution_of(step)
            .map(|stored| &stored.value)
            .ok_or(OperationCommandError::NotFound)
    }

    /// Approves an Execution step for the caller. A destructive skill, or one whose risk is
    /// not known, needs its name typed exactly on the web and can never be approved from
    /// chat. The web judges the risk by the host's current skill list, chat by the risk
    /// stored with the execution; both fail closed.
    pub async fn approve_step(
        &self,
        caller: &Caller,
        op: OperationId,
        step: StepId,
        binding: PlanBinding,
        confirm_text: Option<&str>,
        chat_token: Option<&str>,
    ) -> Result<(), OperationCommandError> {
        let handle = self.handle()?;
        let snapshot = self.snapshot(op).await?;
        let row = Self::execution(&snapshot, step)?;
        let skill = row.contract.skill_name().ok_or_else(|| {
            OperationCommandError::Conflict("only a skill can be approved from here".to_owned())
        })?;
        let risk = match caller.surface() {
            Surface::Web { .. } => risk_of_skill(&self.agents.skills(row.host), &skill),
            Surface::Chat { .. } => row.risk(),
        };
        if !matches!(risk, Some(Risk::Read | Risk::Write)) {
            if caller.is_chat() {
                return Err(OperationCommandError::WebOnly);
            }
            if confirm_text != Some(skill.as_str()) {
                return Err(OperationCommandError::TypedConfirmationRequired);
            }
        }
        let (authorization, provenance) =
            authorization_for(caller, confirm_text, &skill, self.clock.now());
        handle
            .authorize(op, step, authorization, binding, provenance, chat_token)
            .await
            .map_err(OperationCommandError::Operation)
    }

    /// Denies an Execution step for the caller. A chat press carries its card's token, which
    /// the writer checks; the web names none.
    pub async fn deny_step(
        &self,
        caller: &Caller,
        op: OperationId,
        step: StepId,
        chat_token: Option<&str>,
    ) -> Result<(), OperationCommandError> {
        let handle = self.handle()?;
        if let Some(token) = chat_token {
            let approval = approval_for(caller, None, None, self.clock.now());
            return handle
                .deny_from_chat(op, step, approval, token)
                .await
                .map_err(OperationCommandError::Operation);
        }
        let snapshot = self.snapshot(op).await?;
        let row = Self::execution(&snapshot, step)?;
        let skill = row.contract.skill_name().ok_or_else(|| {
            OperationCommandError::Conflict("only a skill can be denied from here".to_owned())
        })?;
        let approval = approval_for(caller, None, Some(&skill), self.clock.now());
        handle
            .deny(op, step, approval)
            .await
            .map_err(OperationCommandError::Operation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn risk_of_skill_treats_a_missing_skill_as_unknown() {
        let name = SkillName::parse("restart_container").unwrap();
        assert_eq!(risk_of_skill(&[], &name), None);
    }
}
