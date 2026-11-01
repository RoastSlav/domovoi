//! What the API returns for auth, hosts, skills, notes, settings, advisories, the digest
//! and events.

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

/// The logged-in user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct UserDto {
    /// User id.
    pub id: String,
    /// Login name.
    pub username: String,
}

/// The current session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct SessionDto {
    /// Who is logged in.
    pub user: UserDto,
    /// Send as `X-CSRF-Token` on every `POST`, `PUT`, `PATCH` and `DELETE`.
    pub csrf_token: String,
}

/// Logging in.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
pub struct LoginRequest {
    /// Login name.
    pub username: String,
    /// Password.
    pub password: String,
}

/// Creating the admin account on first run.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
pub struct SetupRequest {
    /// The token from the setup link the hub logged.
    pub token: String,
    /// Login name, 1 to 64 characters without spaces.
    pub username: String,
    /// Password, at least 12 characters.
    pub password: String,
}

/// A host and its agent's presence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct HostDto {
    /// Host id.
    pub id: String,
    /// Host name.
    pub name: String,
    /// `agent`, `container_agent` or `external`.
    pub kind: String,
    /// Whether the agent is connected now.
    pub connected: bool,
    /// The agent's version, while connected.
    pub version: Option<String>,
    /// Since when it is connected.
    pub connected_since: Option<String>,
    /// When it was last seen, while disconnected.
    pub last_seen: Option<String>,
}

/// An enrollment invite. Shown once; the token is not stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct InviteDto {
    /// The host it is for.
    pub host_id: String,
    /// The one-time enrollment token.
    pub token: String,
    /// When the token stops working.
    pub expires_at: String,
    /// The hub's certificate fingerprint the agent pins.
    pub fingerprint: String,
    /// The command to run on the host.
    pub command: String,
}

/// Whether an agent was revoked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct RevokeDto {
    /// `false` when the host had no live agent.
    pub revoked: bool,
}

/// One declared skill parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ParamDto {
    /// Parameter name.
    pub name: String,
    /// What it means.
    pub description: String,
    /// Whether it must be given.
    pub required: bool,
    /// The type and its limits as JSON, for example `{"type":"integer","min":1,"max":10}`.
    pub kind: serde_json::Value,
}

/// A skill on a host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct SkillDto {
    /// Skill name.
    pub name: String,
    /// What it does.
    pub description: String,
    /// `read`, `write` or `destructive`.
    pub risk: String,
    /// Declared parameters.
    pub params: Vec<ParamDto>,
    /// Checks it is relevant to.
    pub applies_to: Vec<String>,
    /// How long one run may take, in seconds.
    pub timeout_secs: i64,
}

/// A note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct NoteDto {
    /// Note id.
    pub id: String,
    /// The host it is about, by id; absent for global notes.
    pub host_id: Option<String>,
    /// Title.
    pub title: String,
    /// Body in Markdown.
    pub body_markdown: String,
    /// Tags.
    pub tags: Vec<String>,
    /// When it last changed.
    pub updated_at: String,
}

/// Creating or replacing a note.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
pub struct NoteInput {
    /// The host it is about, by name; absent for a global note.
    pub host: Option<String>,
    /// Title, 1 to 200 characters.
    pub title: String,
    /// Body in Markdown, at most 64 KiB.
    pub body_markdown: String,
    /// Tags.
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Which notes to list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct NoteListQuery {
    /// Only notes about this host, by name; absent for global notes.
    pub host: Option<String>,
}

/// A note search.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct NoteSearchQuery {
    /// Words to look for.
    pub q: String,
    /// Also search this host's notes, by name.
    pub host: Option<String>,
}

/// One search hit.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct NoteHitDto {
    /// The note.
    pub note: NoteDto,
    /// The matching text.
    pub snippet: String,
    /// Relevance; higher is better.
    pub score: f64,
}

/// An open advisory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct AdvisoryDto {
    /// Advisory id.
    pub id: String,
    /// The kind, for example `stale_image`.
    pub kind: String,
    /// The host it is about, if any.
    pub host_id: Option<String>,
    /// One line.
    pub title: String,
    /// What to do.
    pub detail: String,
    /// When it was raised.
    pub created_at: String,
}

/// The daily digest as it would be sent now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct DigestPreview {
    /// The day it is for.
    pub date: String,
    /// The rendered Markdown.
    pub markdown: String,
}

/// A change the web UI should show, sent on the event stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct UiEvent {
    /// `incident` or `action`.
    pub kind: String,
    /// What happened, for example `incident_opened` or `action_changed`.
    pub event: String,
    /// The incident concerned.
    pub incident_id: String,
    /// The action concerned, for action events.
    pub action_id: Option<String>,
    /// The new status name, when it changed.
    pub status: Option<String>,
}
