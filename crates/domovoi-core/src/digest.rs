//! The daily digest: one summary per day across all hosts.

use jiff::{SignedDuration, Timestamp, civil::Date};

use crate::advisory::Advisory;
use crate::host::HostName;
use crate::id::{HostId, IncidentId};

/// A fact's value.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum FactValue {
    /// A measurement.
    Number(f64),
    /// A description.
    Text(String),
}

/// One fact a check or skill contributes to the digest, for example pool usage.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DigestFact {
    /// The host it is about.
    pub host: HostId,
    /// Stable key used to compute trends, for example `zfs.tank.used_pct`.
    pub key: String,
    /// Human label.
    pub label: String,
    /// The value.
    pub value: FactValue,
    /// Unit, for example `%`.
    pub unit: Option<String>,
    /// When it was observed.
    pub observed_at: Timestamp,
}

/// How a fact changed over a period.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Trend {
    /// The fact key.
    pub key: String,
    /// Change over `period`.
    pub delta: f64,
    /// The period the delta covers.
    #[serde(serialize_with = "jiff::fmt::serde::duration::friendly::compact::required")]
    pub period: SignedDuration,
    /// Plain-language projection, for example "full in about 40 days".
    pub projection: Option<String>,
}

/// One host's section of the digest.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HostDigest {
    /// The host.
    pub host: HostName,
    /// Nothing changed; render as one line.
    pub quiet: bool,
    /// Changes since the previous digest.
    pub lines: Vec<String>,
    /// Trends worth showing.
    pub trends: Vec<Trend>,
}

/// The daily digest.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Digest {
    /// The local date it covers.
    pub date: Date,
    /// One section per host.
    pub hosts: Vec<HostDigest>,
    /// Incidents still open, with their titles.
    pub open_incidents: Vec<(IncidentId, String)>,
    /// Unresolved hygiene findings.
    pub advisories: Vec<Advisory>,
}
