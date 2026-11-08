//! What the hub knows about a machine, and when it refreshes that picture.
//!
//! A profile is what a machine looks like while it behaves normally: its facts, its
//! containers, listening ports, the read skills it installed and the checks its agent
//! leaves on, plus the raw tool output behind that. The hub collects one by asking its
//! agent to run read tools (see `collect`) and keeps it on the
//! hub only: as JSON under `host.<host id>.profile` in [`SettingsRepo`], and as a [`Note`]
//! tagged `generated` so a note search can find it when its words match. Nothing
//! like it is stored in an agent, and the note is replaced on every refresh, so writing
//! in it is pointless. [`spawn`] runs the minute scheduler that collects one host per
//! tick when it has no profile yet, its agent version or machine facts changed, or the
//! stored profile is a week old.

pub mod collect;
mod render;

use std::sync::Arc;

use domovoi_core::ports::{
    AgentLink, Clock, HostRepo, NoteRepo, Presence, Redactor, SettingsRepo, StoreError,
};
use domovoi_core::{Host, HostId, Note, NoteId};
use domovoi_proto::messages::HostFacts;
use jiff::SignedDuration;
use jiff::Timestamp;
use serde_json::json;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::api::AgentAdmin;
use crate::runtime::every::every;

/// The tag that marks the profile's note as written by the hub, shown as "Generated".
pub const GENERATED_TAG: &str = "generated";

/// The tag that groups the profile notes of every host.
pub const PROFILE_TAG: &str = "host-profile";

/// How often the profile task looks for a host to refresh.
pub const TICK: SignedDuration = SignedDuration::from_mins(1);

/// A profile older than this is collected again even while nothing changed.
pub const STALE_AFTER: SignedDuration = SignedDuration::from_hours(24 * 7);

/// What Domovoi knows about one machine at one moment.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HostProfile {
    /// When the collection started.
    pub collected_at: Timestamp,
    /// The agent version that answered the requests.
    pub agent_version: String,
    /// What the agent said about its machine.
    pub facts: ProfileFacts,
    /// Containers, as the agent's `docker ps` listed them.
    pub containers: Vec<ContainerLine>,
    /// Ports answered outside loopback, one line each.
    pub listeners: Vec<String>,
    /// Skills the agent announced, as `name (risk)`.
    pub skills: Vec<String>,
    /// The checks the agent's own config leaves on.
    pub checks: Vec<String>,
    /// The raw tool output behind the fields above.
    pub sections: Vec<ProfileSection>,
    /// The generated note that carries this profile's text, reused on refreshes.
    pub note: Option<NoteId>,
}

/// The machine facts a profile keeps.
///
/// A copy of the handshake's [`domovoi_proto::messages::HostFacts`] without the
/// boot id: it changes on every reboot, which says nothing a human reads here.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProfileFacts {
    /// The kernel's hostname.
    pub hostname: String,
    /// Operating system name and version.
    pub os: String,
    /// Kernel release.
    pub kernel: String,
    /// CPU architecture.
    pub arch: String,
    /// Logical CPUs.
    pub cpu_count: u32,
    /// Total memory in bytes.
    pub mem_total_bytes: u64,
    /// When the host booted.
    pub boot_time: Timestamp,
}

/// One container as the agent's `docker ps` listed it.
///
/// Every field stays as it read, so a state the agent spells unexpectedly still reaches
/// the note.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ContainerLine {
    /// The container's name.
    pub name: String,
    /// The image it runs.
    pub image: String,
    /// Its run state.
    pub state: String,
    /// Its health check result, or `-`.
    pub health: String,
    /// How often it restarted.
    pub restarts: String,
}

/// One tool output kept in a profile.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProfileSection {
    /// What the tool was asked for, for example `Containers`.
    pub title: String,
    /// The output, redacted and cut, or why it did not answer.
    pub text: String,
    /// Whether the tool answered.
    pub ok: bool,
}

/// Why a profile could not be collected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProfileError {
    /// The host has no connected agent to ask.
    #[error("the agent is not connected")]
    NotConnected,
    /// The store refused the read or the write.
    #[error("{0}")]
    Store(#[from] StoreError),
}

/// Everything the profile task and the API need to collect and save a profile.
#[derive(Clone)]
pub struct ProfileDeps {
    /// The agent requests go through.
    pub link: Arc<dyn AgentLink>,
    /// The handshake's facts and check settings.
    pub agents: Arc<dyn AgentAdmin>,
    /// Where the hosts are listed.
    pub hosts: Arc<dyn HostRepo>,
    /// Where the generated note is written.
    pub notes: Arc<dyn NoteRepo>,
    /// Where the profile itself is stored.
    pub settings: Arc<dyn SettingsRepo>,
    /// Time.
    pub clock: Arc<dyn Clock>,
    /// Secrets are removed before any text is kept.
    pub redactor: Arc<dyn Redactor>,
}

/// The settings key where the profile of `host` is stored.
pub fn profile_key(host: HostId) -> String {
    format!("host.{host}.profile")
}

/// The line left behind where text was cut.
pub(crate) const CUT_MARK: &str = "\n[the text was cut]";

/// Keeps `text` within `limit` bytes, at a character boundary, and says that it was cut.
pub(crate) fn cut(mut text: String, limit: usize) -> String {
    if text.len() <= limit {
        return text;
    }
    let mut keep = limit.saturating_sub(CUT_MARK.len());
    while keep > 0 && !text.is_char_boundary(keep) {
        keep -= 1;
    }
    text.truncate(keep);
    text.push_str(CUT_MARK);
    text
}

/// The profile stored for `host`; a value of another shape is treated as none.
pub async fn stored(deps: &ProfileDeps, host: HostId) -> Result<Option<HostProfile>, ProfileError> {
    let value = deps.settings.get(&profile_key(host)).await?;
    Ok(value.and_then(|value| serde_json::from_value(value).ok()))
}

/// Whether the stored profile must be collected again.
pub fn needs_refresh(
    profile: &HostProfile,
    facts: HostFacts,
    version: &str,
    now: Timestamp,
) -> bool {
    if profile.agent_version != version {
        return true;
    }
    if profile.facts.os != facts.os || profile.facts.kernel != facts.kernel {
        return true;
    }
    if profile.facts.mem_total_bytes != facts.mem_total_bytes
        || profile.facts.cpu_count != facts.cpu_count
    {
        return true;
    }
    profile
        .collected_at
        .checked_add(STALE_AFTER)
        .map_or(true, |due| now >= due)
}

/// The note text of `profile`, rendered as the hub writes it, so the API and the note
/// agree.
#[must_use]
pub fn note_text(host: &Host, profile: &HostProfile) -> String {
    render::markdown(host, profile)
}

/// Collects a profile for `host`, writes its note and saves it.
pub async fn refresh(deps: &ProfileDeps, host: &Host) -> Result<HostProfile, ProfileError> {
    let previous = stored(deps, host.id).await?;
    let mut profile = collect::collect(deps, host).await?;
    let id = previous
        .and_then(|earlier| earlier.note)
        .unwrap_or(NoteId::new());
    profile.note = Some(id);
    let note = Note {
        id,
        host: Some(host.id),
        title: format!("Host profile: {}", host.name),
        body_markdown: render::markdown(host, &profile),
        tags: [PROFILE_TAG, GENERATED_TAG]
            .iter()
            .map(|tag| tag.to_string())
            .collect(),
        updated_at: profile.collected_at,
    };
    deps.notes.upsert(&note).await?;
    deps.settings
        .set(&profile_key(host.id), &json!(profile))
        .await?;
    tracing::info!(
        host = host.name.as_str(),
        sections = profile.sections.len(),
        "host profile collected",
    );
    Ok(profile)
}

/// Collects the profile of every host the hub knows, one per tick.
pub(crate) async fn tick(deps: &ProfileDeps) {
    let hosts = match deps.hosts.list_hosts().await {
        Ok(hosts) => hosts,
        Err(err) => {
            tracing::warn!(%err, "could not list the hosts to profile");
            return;
        }
    };
    for host in hosts {
        let needs = match due(deps, &host).await {
            Ok(needs) => needs,
            Err(err) => {
                tracing::debug!(%err, host = host.name.as_str(), "could not tell whether the host needs a profile");
                continue;
            }
        };
        if !needs {
            continue;
        }
        if let Err(err) = refresh(deps, &host).await {
            tracing::warn!(%err, host = host.name.as_str(), "could not collect the host profile");
            continue;
        }
        return;
    }
}

/// Whether `host` has a connected agent whose profile should be collected again.
async fn due(deps: &ProfileDeps, host: &Host) -> Result<bool, ProfileError> {
    let version = match deps.link.presence(host.id) {
        Presence::Connected { version, .. } => version,
        Presence::Disconnected { .. } => return Ok(false),
    };
    let facts = match deps.agents.facts(host.id) {
        Some(facts) => facts,
        None => return Ok(false),
    };
    let previous = stored(deps, host.id).await?;
    let needs = match previous {
        None => true,
        Some(profile) => needs_refresh(&profile, facts, &version, deps.clock.now()),
    };
    Ok(needs)
}

/// The profile task: every minute, refresh at most one host that needs it.
pub fn spawn(deps: ProfileDeps, shutdown: CancellationToken) -> JoinHandle<()> {
    let clock = deps.clock.clone();
    tokio::spawn(every(clock, TICK, shutdown, move || {
        let deps = deps.clone();
        async move { tick(&deps).await }
    }))
}
