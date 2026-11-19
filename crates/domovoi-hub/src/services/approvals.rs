//! Approving and denying proposed actions, for both surfaces.

use std::sync::Arc;

use domovoi_core::ports::{ActionRepo, Clock};
use domovoi_core::{ActionId, ActionRequest, PlanBinding, Risk};
use domovoi_engine::{ActionHandle, ApprovalError};

use super::Caller;
use crate::auth::approval::{approval_for, authorization_for};

/// What an approval carries besides who asked.
pub struct ApproveInput<'a> {
    /// The plan the person saw: the full hash from the web, a prefix from a chat card.
    pub binding: PlanBinding,
    /// A chat card's single-use token.
    pub chat_token: Option<&'a str>,
    /// What the person typed to confirm a destructive skill.
    pub confirm_text: Option<&'a str>,
}

/// Why an approval was refused.
#[derive(Debug)]
pub enum ApproveError {
    /// A chat caller tried to approve a destructive action; only the web may.
    WebOnly,
    /// The approval service refused.
    Approval(ApprovalError),
}

/// Approves and denies legacy actions.
#[derive(Clone)]
pub struct ApprovalService {
    pub(super) actions: Arc<dyn ActionRepo>,
    pub(super) handle: ActionHandle,
    pub(super) clock: Arc<dyn Clock>,
}

impl ApprovalService {
    /// Whether the action service is still running; `/readyz` reports it.
    #[must_use]
    pub fn is_alive(&self) -> bool {
        self.handle.is_alive()
    }

    /// The action `id`, or `NotFound`.
    pub async fn find(&self, id: ActionId) -> Result<ActionRequest, ApprovalError> {
        match self.actions.get(id).await {
            Ok(Some(found)) => Ok(found),
            Ok(None) => Err(ApprovalError::NotFound(id)),
            Err(err) => Err(ApprovalError::Store(err.to_string())),
        }
    }

    /// Approves `action` for the caller.
    pub async fn approve(
        &self,
        caller: &Caller,
        action: &ActionRequest,
        input: ApproveInput<'_>,
    ) -> Result<ActionRequest, ApproveError> {
        if caller.is_chat() && action.risk() == Risk::Destructive {
            return Err(ApproveError::WebOnly);
        }
        let (authorization, provenance) =
            authorization_for(caller, input.confirm_text, action.skill(), self.clock.now());
        self.handle
            .approve(
                action.id(),
                authorization,
                input.chat_token,
                input.binding,
                provenance,
            )
            .await
            .map_err(ApproveError::Approval)
    }

    /// Denies `action` for the caller.
    pub async fn deny(
        &self,
        caller: &Caller,
        action: &ActionRequest,
    ) -> Result<ActionRequest, ApprovalError> {
        let approval = approval_for(caller, None, Some(action.skill()), self.clock.now());
        self.handle.deny(action.id(), approval).await
    }
}
