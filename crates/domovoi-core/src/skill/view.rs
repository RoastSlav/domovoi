//! What a skill needs to see of its host, and the built-in reads the agent can answer in its
//! place when the skill runner's account cannot see it.

use serde::{Deserialize, Serialize};

/// A part of the host that the skill runner's account cannot see (three-identities layout).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostNeed {
    /// The service manager (the system bus).
    Services,
    /// Other accounts' processes.
    Processes,
    /// The system journal.
    Journal,
    /// Disk devices and the kernel log.
    Hardware,
    /// Directories under `/home`, `/root` or `/var/lib`.
    Files,
}

impl HostNeed {
    /// The words an owner reads.
    #[must_use]
    pub const fn describe(self) -> &'static str {
        match self {
            Self::Services => "the service manager",
            Self::Processes => "other accounts' processes",
            Self::Journal => "the system journal",
            Self::Hardware => "disks and the kernel log",
            Self::Files => "directories under /home, /root or /var/lib",
        }
    }
}

/// A fixed read the agent runs itself, with the same output sections as the starter skill,
/// when the skill runs behind the skill runner. It never changes the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRead {
    /// Running and failed services, and timers.
    Services,
    /// The top processes by CPU and by memory.
    Processes,
    /// Warnings and errors per program over 24 hours, and the newest errors.
    LogSummary,
    /// One unit's errors; takes the parameters `unit` and `minutes`.
    JournalErrors,
    /// Temperatures, kernel errors and disk health.
    Hardware,
}

impl AgentRead {
    /// The need this read stands in for.
    #[must_use]
    pub const fn covers(self) -> HostNeed {
        match self {
            Self::Services => HostNeed::Services,
            Self::Processes => HostNeed::Processes,
            Self::LogSummary | Self::JournalErrors => HostNeed::Journal,
            Self::Hardware => HostNeed::Hardware,
        }
    }

    /// The name written in a manifest.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Services => "services",
            Self::Processes => "processes",
            Self::LogSummary => "log_summary",
            Self::JournalErrors => "journal_errors",
            Self::Hardware => "hardware",
        }
    }

    /// The parameters the manifest must declare for this read.
    #[must_use]
    pub const fn params(self) -> &'static [&'static str] {
        match self {
            Self::JournalErrors => &["unit", "minutes"],
            _ => &[],
        }
    }
}
