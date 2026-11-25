//! Considering a proposed execution for a policy permit.
//!
//! The handler reads the step and its row from the store, asks the permit issuer port, and, only
//! when the issuer hands back an authorization, asks the writer to record it. The engine names the
//! authorization type but never builds one: the issuer is the one place a permit is made. When
//! nothing is issued the handler changes nothing, and the approval offer stands.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::ports::{
    ClaimToken, EffectRecord, NotIssued, PermitOutcome, PermitRefusal, PermitRequest,
};

use super::dispatch::pending_execution;
use super::{Address, address, ask, finish, held, load, outcome, unreadable};
use crate::effects::{EffectHandler, EffectOutcome};
use crate::operations::writer::{OpShared, OperationCommand};

/// Offers a freshly proposed execution to the permit issuer.
pub(super) struct PolicyHandler {
    shared: Arc<OpShared>,
}

impl PolicyHandler {
    /// A handler over the operation service's writer and ports.
    pub(super) fn new(shared: Arc<OpShared>) -> Self {
        Self { shared }
    }
}

#[async_trait]
impl EffectHandler for PolicyHandler {
    async fn handle(&self, effect: &EffectRecord, claim: ClaimToken) -> EffectOutcome {
        let shared = &*self.shared;
        let held = held(effect, claim);
        let Some(Address {
            operation,
            step: Some(step),
        }) = address(effect)
        else {
            return unreadable();
        };
        let snapshot = match load(shared, operation).await {
            Ok(snapshot) => snapshot,
            Err(done) => return done,
        };
        // A step that is no longer waiting, or that a person already approved, needs nothing.
        if !pending_execution(&snapshot, step) {
            return finish(shared, held).await;
        }
        let Some(row) = snapshot
            .execution_of(step)
            .map(|stored| stored.value.clone())
        else {
            return EffectOutcome::Dead {
                error: "the execution has no row".to_owned(),
            };
        };
        if row.is_authorized() {
            return finish(shared, held).await;
        }
        let Some(autonomy) = &shared.deps.autonomy else {
            return finish(shared, held).await;
        };
        let request = PermitRequest {
            execution: row,
            pinned: snapshot.operation.pinned.clone(),
        };
        match autonomy.issuer.issue(request).await {
            PermitOutcome::Issued(grant) => outcome(
                ask(shared, |reply| OperationCommand::AuthorizeByPolicy {
                    held,
                    operation,
                    step,
                    authorization: grant.authorization,
                    decision: grant.decision,
                    reply,
                })
                .await,
            ),
            // The inputs could not be read: worth another attempt, and after the last the
            // approval offer stands.
            PermitOutcome::NotIssued(NotIssued::Withheld(PermitRefusal::InputsUnavailable)) => {
                EffectOutcome::Retry {
                    error: "the policy inputs could not be read".to_owned(),
                }
            }
            PermitOutcome::NotIssued(_) => finish(shared, held).await,
        }
    }
}
