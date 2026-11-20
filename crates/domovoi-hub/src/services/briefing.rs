//! What the hub tells a person about the day: the digest, and a host's profile summary.

use std::sync::Arc;

use domovoi_config::HubConfig;
use domovoi_core::ports::{
    AdvisoryRepo, Clock, HostRepo, IncidentRepo, MetricSampleRepo, StoreError,
};
use domovoi_core::{Host, HostName};
use jiff::Timestamp;

use crate::digest::{DigestDeps, DigestError};
use crate::profile::{ProfileDeps, ProfileError, render::summary, stored};

/// One host's part of the digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BriefingHost {
    /// The host.
    pub host: HostName,
    /// Whether nothing happened on it.
    pub quiet: bool,
    /// What happened, one line each.
    pub lines: Vec<String>,
}

/// Today's digest as it would be sent now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Briefing {
    /// The local date it covers.
    pub date: String,
    /// The Markdown that is sent.
    pub markdown: String,
    /// When the next scheduled digest goes out, when digests are on.
    pub next_send_at: Option<Timestamp>,
    /// The per-host parts the Markdown is made from.
    pub hosts: Vec<BriefingHost>,
}

/// Why a digest could not be assembled.
#[derive(Debug, thiserror::Error)]
pub enum BriefingError {
    /// The store failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The start of the local digest day could not be found.
    #[error("could not find the start of the local digest day: {0}")]
    DayStart(String),
}

/// Builds digests and profile summaries.
#[derive(Clone)]
pub struct BriefingService {
    pub(super) config: Arc<HubConfig>,
    pub(super) clock: Arc<dyn Clock>,
    pub(super) hosts: Arc<dyn HostRepo>,
    pub(super) facts: Arc<dyn MetricSampleRepo>,
    pub(super) incidents: Arc<dyn IncidentRepo>,
    pub(super) advisories: Arc<dyn AdvisoryRepo>,
    pub(super) profiles: Option<ProfileDeps>,
}

impl BriefingService {
    /// Today's digest, built now.
    pub async fn digest(&self) -> Result<Briefing, BriefingError> {
        let digest = &self.config.notify.digest;
        let schedule = domovoi_engine::digest::DigestService {
            enabled: digest.enabled,
            time: digest.time.time(),
            tz: self.config.hub.timezone.zone().clone(),
        };
        let rendered = crate::digest::build(&DigestDeps {
            config: self.config.clone(),
            clock: self.clock.clone(),
            hosts: self.hosts.clone(),
            facts: self.facts.clone(),
            incidents: self.incidents.clone(),
            advisories: self.advisories.clone(),
        })
        .await
        .map_err(|err| match err {
            DigestError::Store(err) => BriefingError::Store(err),
            DigestError::DayStart(err) => BriefingError::DayStart(err),
        })?;
        Ok(Briefing {
            date: rendered.date,
            markdown: rendered.markdown,
            next_send_at: schedule.next_due(self.clock.now()),
            hosts: rendered
                .hosts
                .into_iter()
                .map(|host| BriefingHost {
                    host: host.host,
                    quiet: host.quiet,
                    lines: host.lines,
                })
                .collect(),
        })
    }

    /// The top of `host`'s profile note: the summary and the recent changes, without the
    /// raw tool output. Empty when there is nothing to show: the hub has not collected a
    /// profile yet, or it was built without profile collection.
    pub async fn profile_summary(&self, host: &Host) -> Result<String, ProfileError> {
        let Some(deps) = &self.profiles else {
            return Ok(String::new());
        };
        Ok(match stored(deps, host.id).await? {
            None => String::new(),
            Some(profile) => summary(host, &profile),
        })
    }
}
