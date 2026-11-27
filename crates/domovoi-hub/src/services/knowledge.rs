//! The knowledge use cases: what the owner can assert, accept, retract, verify and confirm,
//! and how the facts, relations and entities are read.
//!
//! Every write takes a [`Caller`], so only an authenticated person can write an owner fact.
//! Changes that touch a safety-critical predicate, and every management-path confirmation,
//! ask for the password again through the login limiter.

use std::sync::Arc;

use domovoi_core::id::{EntityId, FactRecordId, HostId, RelationId};
use domovoi_core::knowledge::{Entity, Fact, Relation};
use domovoi_core::ports::{
    FactFilter, FactHit, KnowledgeRead, Page, Paged, RelationFilter, StoreError, Versioned,
};
use domovoi_engine::knowledge::{
    FactDraft, KnowledgeError, KnowledgeService, ManagementPathView, Recorded, RelationDraft,
};

use super::Caller;
use super::policy_types::StepUp;
use crate::auth::{AuthError, AuthState};

/// What a refused step-up says when no password came with the request.
pub const KNOWLEDGE_PASSWORD_REQUIRED: &str = "this change needs your password";

/// Why a knowledge command or read was refused.
#[derive(Debug, thiserror::Error)]
pub enum KnowledgeCommandError {
    /// A step-up was needed and no password came with it.
    #[error("this change needs your password")]
    PasswordRequired,
    /// The password is wrong.
    #[error("wrong password")]
    BadPassword,
    /// Too many wrong passwords from this address.
    #[error("too many attempts; try again in {retry_after_secs} s")]
    TooMany {
        /// Seconds until the next attempt.
        retry_after_secs: i64,
    },
    /// The knowledge service refused or failed.
    #[error("{0}")]
    Knowledge(#[from] KnowledgeError),
}

impl From<StoreError> for KnowledgeCommandError {
    fn from(error: StoreError) -> Self {
        Self::Knowledge(KnowledgeError::Store(error))
    }
}

/// The knowledge use cases.
#[derive(Clone)]
pub struct KnowledgeCommands {
    service: Arc<KnowledgeService>,
    read: Arc<dyn KnowledgeRead>,
    auth: Arc<AuthState>,
}

impl KnowledgeCommands {
    /// The use cases over `service`, reading through `read` and asking `auth` for passwords.
    #[must_use]
    pub fn new(
        service: Arc<KnowledgeService>,
        read: Arc<dyn KnowledgeRead>,
        auth: Arc<AuthState>,
    ) -> Self {
        Self {
            service,
            read,
            auth,
        }
    }

    /// Whether a management-path coverage check can currently succeed.
    #[must_use]
    pub fn coverage_available(&self) -> bool {
        self.service.coverage_available()
    }

    /// Asks for the password again. It shares the login limiter.
    async fn step_up(
        &self,
        caller: &Caller,
        proof: StepUp<'_>,
    ) -> Result<(), KnowledgeCommandError> {
        let Some(password) = proof.password else {
            return Err(KnowledgeCommandError::PasswordRequired);
        };
        match self
            .auth
            .confirm_password(proof.ip, caller.user(), password)
            .await
        {
            Ok(()) => Ok(()),
            Err(AuthError::LockedOut { retry_after_secs }) => {
                Err(KnowledgeCommandError::TooMany { retry_after_secs })
            }
            Err(_) => Err(KnowledgeCommandError::BadPassword),
        }
    }

    /// The stored fact, or `NotFound`.
    async fn stored(&self, id: FactRecordId) -> Result<Versioned<Fact>, KnowledgeCommandError> {
        self.read
            .fact(id)
            .await?
            .ok_or(KnowledgeCommandError::Knowledge(KnowledgeError::NotFound))
    }

    /// Records a fact the owner states. The source is always the owner.
    pub async fn assert_fact(
        &self,
        caller: &Caller,
        mut draft: FactDraft,
        proof: StepUp<'_>,
    ) -> Result<Recorded, KnowledgeCommandError> {
        draft.source = domovoi_core::knowledge::FactSource::User;
        // Say that a value cannot be saved before asking for a password to save it.
        self.service.check_owner_draft(&draft)?;
        if draft.predicate.is_safety_critical() {
            self.step_up(caller, proof).await?;
        }
        Ok(self.service.assert_fact(draft, caller.user()).await?)
    }

    /// Turns a non-owner fact into the owner's.
    pub async fn accept(
        &self,
        caller: &Caller,
        id: FactRecordId,
        expected: u64,
        proof: StepUp<'_>,
    ) -> Result<Recorded, KnowledgeCommandError> {
        let fact = self.stored(id).await?;
        if fact.value.predicate.is_safety_critical() {
            self.step_up(caller, proof).await?;
        }
        Ok(self.service.accept(id, expected, caller.user()).await?)
    }

    /// Withdraws a fact. It only removes trust, so it needs no password.
    pub async fn retract(
        &self,
        caller: &Caller,
        id: FactRecordId,
        expected: u64,
    ) -> Result<Versioned<Fact>, KnowledgeCommandError> {
        Ok(self.service.retract(id, expected, caller.user()).await?)
    }

    /// Says a fact still holds.
    pub async fn verify(
        &self,
        caller: &Caller,
        id: FactRecordId,
        expected: u64,
        proof: StepUp<'_>,
    ) -> Result<Versioned<Fact>, KnowledgeCommandError> {
        let fact = self.stored(id).await?;
        if fact.value.predicate.is_safety_critical() {
            self.step_up(caller, proof).await?;
        }
        Ok(self.service.verify(id, expected, caller.user()).await?)
    }

    /// Confirms a management path against the helper's verified coverage. Always asks for
    /// the password.
    pub async fn confirm(
        &self,
        caller: &Caller,
        id: FactRecordId,
        expected: u64,
        proof: StepUp<'_>,
    ) -> Result<ManagementPathView, KnowledgeCommandError> {
        // Say that a fact cannot be confirmed before asking for a password to confirm it.
        let fact = self.stored(id).await?;
        if fact.value.predicate != domovoi_core::knowledge::Predicate::ManagementPath {
            return Err(KnowledgeError::InvalidInput(
                "only a management path can be confirmed".to_owned(),
            )
            .into());
        }
        self.step_up(caller, proof).await?;
        Ok(self
            .service
            .confirm_management_path(id, expected, caller.user())
            .await?)
    }

    /// One fact with its version.
    pub async fn fact(&self, id: FactRecordId) -> Result<Versioned<Fact>, KnowledgeCommandError> {
        self.stored(id).await
    }

    /// A page of facts, newest first.
    pub async fn facts(
        &self,
        filter: &FactFilter,
        page: &Page,
    ) -> Result<Paged<Fact>, KnowledgeCommandError> {
        Ok(self.service.facts(filter, page).await?)
    }

    /// The listed facts with the version each is stored at. A fact that vanished between the
    /// page read and this one (a purge) is left out.
    pub async fn with_versions(
        &self,
        facts: &[Fact],
    ) -> Result<Vec<Versioned<Fact>>, KnowledgeCommandError> {
        let mut out = Vec::with_capacity(facts.len());
        for fact in facts {
            if let Some(stored) = self.read.fact(fact.id).await? {
                out.push(stored);
            }
        }
        Ok(out)
    }

    /// The listed relations with the version each is stored at.
    pub async fn relations_with_versions(
        &self,
        relations: &[Relation],
    ) -> Result<Vec<Versioned<Relation>>, KnowledgeCommandError> {
        let mut out = Vec::with_capacity(relations.len());
        for relation in relations {
            if let Some(stored) = self.read.relation(relation.id).await? {
                out.push(stored);
            }
        }
        Ok(out)
    }

    /// Full-text search over live facts.
    pub async fn search(
        &self,
        query: &str,
        host: Option<HostId>,
        limit: u16,
    ) -> Result<Vec<FactHit>, KnowledgeCommandError> {
        Ok(self.service.search(query, host, limit).await?)
    }

    /// The host's management paths with their confirmations.
    pub async fn management_paths(
        &self,
        host: HostId,
    ) -> Result<Vec<ManagementPathView>, KnowledgeCommandError> {
        Ok(self.service.management_paths(host).await?)
    }

    /// A page of relations, newest first.
    pub async fn relations(
        &self,
        filter: &RelationFilter,
        page: &Page,
    ) -> Result<Paged<Relation>, KnowledgeCommandError> {
        Ok(self.read.relations(filter, page).await?)
    }

    /// Whether an owner-made entity with this id exists.
    pub async fn entity_exists(
        &self,
        id: domovoi_core::EntityId,
    ) -> Result<bool, KnowledgeCommandError> {
        Ok(self.read.entity(id).await?.is_some())
    }

    /// Records a relation the owner states.
    pub async fn assert_relation(
        &self,
        caller: &Caller,
        mut draft: RelationDraft,
    ) -> Result<Versioned<Relation>, KnowledgeCommandError> {
        draft.source = domovoi_core::knowledge::FactSource::User;
        Ok(self.service.assert_relation(draft, caller.user()).await?)
    }

    /// Withdraws a relation.
    pub async fn retract_relation(
        &self,
        caller: &Caller,
        id: RelationId,
        expected: u64,
    ) -> Result<Versioned<Relation>, KnowledgeCommandError> {
        Ok(self
            .service
            .retract_relation(id, expected, caller.user())
            .await?)
    }

    /// Every entity.
    pub async fn entities(&self) -> Result<Vec<Versioned<Entity>>, KnowledgeCommandError> {
        Ok(self.read.entities().await?)
    }

    /// Creates an entity.
    pub async fn create_entity(
        &self,
        caller: &Caller,
        kind: &str,
        name: &str,
    ) -> Result<Versioned<Entity>, KnowledgeCommandError> {
        Ok(self
            .service
            .create_entity(kind, name, caller.user())
            .await?)
    }

    /// Renames an entity.
    pub async fn rename_entity(
        &self,
        caller: &Caller,
        id: EntityId,
        expected: u64,
        name: &str,
    ) -> Result<Versioned<Entity>, KnowledgeCommandError> {
        Ok(self
            .service
            .rename_entity(id, expected, name, caller.user())
            .await?)
    }

    /// Deletes an entity nothing refers to.
    pub async fn delete_entity(
        &self,
        caller: &Caller,
        id: EntityId,
        expected: u64,
    ) -> Result<(), KnowledgeCommandError> {
        Ok(self
            .service
            .delete_entity(id, expected, caller.user())
            .await?)
    }
}
