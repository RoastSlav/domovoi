//! What the hub knows about a machine, and when it refreshes that picture.
//!
//! A profile is what a machine looks like while it behaves normally: its facts, its
//! containers, listening ports, the read skills it installed and the checks its agent
//! leaves on, plus the raw tool output behind that. The hub collects one by asking its
//! agent to run read tools (see `collect`) and keeps it on the
//! hub only: as JSON under `host.<host id>.profile` in [`SettingsRepo`], plus its rendered
//! text under `host.<host id>.profile_text` for triage. It is not a note: a person's notes
//! stay theirs, and the profile has its own page and its own search group. Nothing like it is
//! stored in an agent. Each refresh also records what came, went or changed since the stored
//! profile, so the page can show what is new since the machine was last normal.
//! [`spawn`] runs the minute scheduler that collects one host per
//! tick when it has no profile yet, its agent version or machine facts changed, or the
//! stored profile is a week old.

pub mod backfill;
pub mod collect;
pub mod diff;
pub(crate) mod render;

use std::sync::Arc;

use domovoi_core::ports::{
    Clock, HostRepo, NoteRepo, Presence, Redactor, SettingsRepo, StoreError,
};
use domovoi_core::{Host, HostId};
use domovoi_engine::{Evidence, HostCatalog};
use domovoi_proto::messages::HostFacts;
use jiff::SignedDuration;
use jiff::Timestamp;
use serde_json::json;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::agent_server::AgentAdmin;
use crate::runtime::every::every;

/// The tag older hubs put on the note they wrote from a profile, with [`PROFILE_TAG`]. Such
/// notes are removed at start; nothing writes them any more.
const LEGACY_GENERATED_TAG: &str = "generated";

/// The second tag of those older notes.
const LEGACY_PROFILE_TAG: &str = "host-profile";

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
    /// The last changes between refreshes, newest first, each one dated. Kept so a person
    /// and the model can tell what is new since the machine was last normal.
    #[serde(default)]
    pub changes: Vec<String>,
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
    /// The one path to the agent's tool output.
    pub evidence: Evidence,
    /// The agent sessions and skills read beside it.
    pub catalog: HostCatalog,
    /// The capability registry: the skills the owner enables on each host.
    pub capabilities: Arc<dyn domovoi_core::ports::CapabilityRegistry>,
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
    /// Where the machine description is recorded as a fact; `None` leaves profiles as notes.
    pub knowledge: Option<Arc<domovoi_engine::knowledge::KnowledgeService>>,
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

/// Collects a profile for `host` and saves it with its text. The changes since the profile
/// stored before are kept as dated lines, newest first, so the page can show what is new on
/// the machine.
pub async fn refresh(deps: &ProfileDeps, host: &Host) -> Result<HostProfile, ProfileError> {
    let previous = stored(deps, host.id).await?;
    let mut profile = collect::collect(deps, host).await?;
    profile.changes = diff::recent_changes(&previous, profile.collected_at, &profile);
    // The text triage reads is kept beside the profile, not as a note: a person's notes stay
    // theirs.
    let body = render::markdown(host, &profile);
    deps.settings
        .set(
            &domovoi_core::profile_text_key(host.id),
            &json!({ "markdown": body, "collected_at": profile.collected_at }),
        )
        .await?;
    deps.settings
        .set(&profile_key(host.id), &json!(profile))
        .await?;
    if let Some(knowledge) = &deps.knowledge {
        backfill::observe(knowledge, host.id, &profile).await;
    }
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
    let version = match deps.catalog.presence(host.id) {
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

/// Moves what older hubs kept as a note into the profile's own store, once at start: removes the
/// generated `Host profile: <name>` note of every host, and writes the stored text triage reads
/// for each host that has a profile but no text yet. Safe to run again.
pub async fn migrate(deps: &ProfileDeps) {
    let hosts = match deps.hosts.list_hosts().await {
        Ok(hosts) => hosts,
        Err(err) => {
            tracing::warn!(%err, "could not list the hosts to move host profiles for");
            return;
        }
    };
    for host in hosts {
        match deps.notes.list(Some(host.id)).await {
            Ok(notes) => {
                for note in notes.into_iter().filter(|note| {
                    [LEGACY_GENERATED_TAG, LEGACY_PROFILE_TAG]
                        .iter()
                        .all(|tag| note.tags.iter().any(|t| t == tag))
                }) {
                    match deps.notes.delete(note.id).await {
                        Ok(()) => tracing::info!(
                            host = host.name.as_str(),
                            "removed the generated host profile note; the profile has its own page"
                        ),
                        Err(err) => {
                            tracing::warn!(%err, "could not remove a generated profile note")
                        }
                    }
                }
            }
            Err(err) => tracing::warn!(%err, "could not list notes to remove generated profiles"),
        }
        let text_key = domovoi_core::profile_text_key(host.id);
        let has_text = matches!(deps.settings.get(&text_key).await, Ok(Some(_)));
        if has_text {
            continue;
        }
        if let Ok(Some(profile)) = stored(deps, host.id).await {
            let body = render::markdown(&host, &profile);
            if let Err(err) = deps
                .settings
                .set(
                    &text_key,
                    &json!({ "markdown": body, "collected_at": profile.collected_at }),
                )
                .await
            {
                tracing::warn!(%err, "could not store the host profile text");
            }
        }
    }
}

/// The profile task: every minute, refresh at most one host that needs it.
pub fn spawn(deps: ProfileDeps, shutdown: CancellationToken) -> JoinHandle<()> {
    let clock = deps.clock.clone();
    tokio::spawn(async move {
        migrate(&deps).await;
        every(clock, TICK, shutdown, move || {
            let deps = deps.clone();
            async move { tick(&deps).await }
        })
        .await;
    })
}
