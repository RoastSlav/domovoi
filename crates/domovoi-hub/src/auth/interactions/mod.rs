//! The chat interaction authorizer: the hub's [`InteractionSink`]. It alone decides whether
//! a chat user may do anything (ARCHITECTURE.md §13).
//!
//! - Only platform users on the allowlist are heard; everyone else gets "Not allowed".
//! - Chat acts as the hub's admin account, with the chat user recorded in the approval and
//!   the audit log. Before the admin exists, chat does nothing.
//! - Only a button press can approve, and never a destructive action (INV-1, INV-6). Thread
//!   messages become follow-up questions; their text never reaches the approval path.

mod buttons;
mod commands;

use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::ports::{
    AuditEvent, AuditLog, ChatGateway, InteractionSink, NotifyStateRepo, ThreadRef, UserRepo,
};
use domovoi_core::{IncidentId, Interaction, InteractionKind, InteractionReply, User};
use serde_json::Value;

use crate::api::ApiState;

/// What a user who is not on the allowlist is told.
pub const NOT_ALLOWED: &str = "Not allowed";

/// What chat users are told before the admin account exists.
pub const SETUP_FIRST: &str = "Finish setup in the web UI first";

/// What the authorizer needs besides the API's repositories and handles.
pub struct ChatDeps {
    /// The admin account chat acts as.
    pub users: Arc<dyn UserRepo>,
    /// Where each incident's card and thread are.
    pub notify: Arc<dyn NotifyStateRepo>,
    /// The audit log.
    pub audit: Arc<dyn AuditLog>,
    /// The chat platform, for posting in threads.
    pub gateway: Arc<dyn ChatGateway>,
    /// Platform user ids allowed to act, for example `[discord] allowed_user_ids`.
    pub allowed: BTreeSet<String>,
}

/// An accepted interaction: the reply and what goes in the audit log.
pub(crate) struct Accepted {
    /// The confirmation shown to the user.
    message: Option<String>,
    /// The audit kind after `chat.`, for example `ack`.
    kind: &'static str,
    /// What it was done to, for example `incident:<id>`.
    subject: String,
    /// Detail for the audit log; never tokens or message text.
    detail: Value,
}

impl Accepted {
    pub(crate) fn new(kind: &'static str, subject: String, message: impl Into<String>) -> Self {
        Self {
            message: Some(message.into()),
            kind,
            subject,
            detail: Value::Null,
        }
    }

    pub(crate) fn with_detail(mut self, detail: Value) -> Self {
        self.detail = detail;
        self
    }

    pub(crate) fn quiet(mut self) -> Self {
        self.message = None;
        self
    }
}

/// Who asked: the chat user and the hub account they act as.
pub(crate) struct Actor<'a> {
    pub(crate) platform: &'a str,
    pub(crate) platform_user: &'a str,
    pub(crate) admin: User,
}

/// The hub's [`InteractionSink`] for chat gateways.
pub struct InteractionAuthorizer {
    pub(crate) api: ApiState,
    pub(crate) chat: ChatDeps,
}

impl std::fmt::Debug for InteractionAuthorizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InteractionAuthorizer")
            .field("allowed", &self.chat.allowed.len())
            .finish_non_exhaustive()
    }
}

impl InteractionAuthorizer {
    /// An authorizer acting through `api`'s services, for the chat platform in `chat`.
    #[must_use]
    pub fn new(api: &ApiState, chat: ChatDeps) -> Self {
        Self {
            api: api.clone(),
            chat,
        }
    }

    /// The thread of `incident` on the gateway's platform: the recorded one, or the card's
    /// own id (a thread started from a message shares its id).
    pub(crate) async fn thread_of(&self, incident: IncidentId) -> Option<ThreadRef> {
        let state = self.chat.notify.get(incident).await.ok()??;
        let refs = state.messages.get(self.chat.gateway.platform())?;
        Some(refs.thread.clone().unwrap_or_else(|| ThreadRef {
            platform: refs.message.platform.clone(),
            id: refs.message.id.clone(),
        }))
    }

    /// Posts `text` in `incident`'s thread; `false` when there is none or posting failed.
    pub(crate) async fn post_in_thread(&self, incident: IncidentId, text: &str) -> bool {
        let Some(thread) = self.thread_of(incident).await else {
            return false;
        };
        match self.chat.gateway.reply(&thread, text).await {
            Ok(()) => true,
            Err(err) => {
                tracing::warn!(%err, %incident, "could not post in the incident thread");
                false
            }
        }
    }

    async fn follow_up(
        &self,
        actor: &Actor<'_>,
        incident: IncidentId,
        question: &str,
    ) -> Result<Accepted, String> {
        let subject = format!("incident:{incident}");
        let answer = match &self.api.deps().triage {
            None => "Triage is off, so there is no one to ask.".to_owned(),
            Some(triage) => match triage.follow_up(incident, question, actor.admin.id).await {
                Ok(answer) => answer,
                Err(err) => format!("I could not answer that: {err}"),
            },
        };
        let posted = self.post_in_thread(incident, &answer).await;
        let accepted = Accepted::new("follow_up", subject, answer);
        Ok(if posted { accepted.quiet() } else { accepted })
    }

    async fn audit(&self, actor: &Actor<'_>, accepted: &Accepted) {
        let event = AuditEvent {
            at: self.api.deps().clock.now(),
            actor: format!("chat:{}:{}", actor.platform, actor.platform_user),
            kind: format!("chat.{}", accepted.kind),
            subject: accepted.subject.clone(),
            detail: accepted.detail.clone(),
        };
        if let Err(err) = self.chat.audit.append(&event).await {
            tracing::error!(%err, "could not audit a chat interaction");
        }
    }

    async fn handle(&self, actor: &Actor<'_>, kind: &InteractionKind) -> Result<Accepted, String> {
        match kind {
            InteractionKind::Button { button, incident } => {
                self.button(actor, *incident, button).await
            }
            InteractionKind::ThreadMessage { incident, text } => {
                self.follow_up(actor, *incident, text).await
            }
            InteractionKind::Command { name, args } => self.command(name, args, actor).await,
        }
    }
}

#[async_trait]
impl InteractionSink for InteractionAuthorizer {
    async fn deliver(&self, i: Interaction) -> InteractionReply {
        if !self.chat.allowed.contains(&i.platform_user) {
            tracing::info!(platform = %i.platform, user = %i.platform_user, "chat user not allowed");
            return InteractionReply::Rejected {
                reason: NOT_ALLOWED.to_owned(),
            };
        }
        let admin = match self.chat.users.first_user().await {
            Ok(Some(admin)) => admin,
            Ok(None) => {
                return InteractionReply::Rejected {
                    reason: SETUP_FIRST.to_owned(),
                };
            }
            Err(err) => {
                tracing::error!(%err, "could not load the admin account");
                return InteractionReply::Rejected {
                    reason: "Something went wrong; try again".to_owned(),
                };
            }
        };
        let actor = Actor {
            platform: &i.platform,
            platform_user: &i.platform_user,
            admin,
        };
        match self.handle(&actor, &i.kind).await {
            Ok(accepted) => {
                self.audit(&actor, &accepted).await;
                InteractionReply::Accepted {
                    message: accepted.message,
                }
            }
            Err(reason) => InteractionReply::Rejected { reason },
        }
    }
}
