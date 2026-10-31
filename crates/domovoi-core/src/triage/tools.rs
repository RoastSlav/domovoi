//! The closed catalogue of read-only tools the triage model may call.

/// A read-only tool the model may call during triage.
///
/// This enum is closed on purpose (INV-1, ADR 0007): there is no variant that approves,
/// executes a write skill or changes state. Gate tests pin its contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReadTool {
    /// List containers.
    DockerPs,
    /// Tail one container's logs.
    DockerLogs,
    /// Inspect one container.
    DockerInspect,
    /// Recent Docker events.
    DockerEvents,
    /// CPU, memory, disk and uptime summary.
    HostOverview,
    /// Read the systemd journal.
    Journal,
    /// Listening TCP and UDP sockets.
    ListeningPorts,
    /// SMART health of one disk.
    SmartReport,
    /// Past incidents with the same fingerprint.
    IncidentHistory,
    /// Search runbook notes.
    NotesSearch,
    /// Run a skill whose manifest declares `risk = "read"`.
    RunReadSkill,
}

impl ReadTool {
    /// Every tool, in catalogue order.
    pub const ALL: [ReadTool; 11] = [
        ReadTool::DockerPs,
        ReadTool::DockerLogs,
        ReadTool::DockerInspect,
        ReadTool::DockerEvents,
        ReadTool::HostOverview,
        ReadTool::Journal,
        ReadTool::ListeningPorts,
        ReadTool::SmartReport,
        ReadTool::IncidentHistory,
        ReadTool::NotesSearch,
        ReadTool::RunReadSkill,
    ];

    /// The snake_case name the model calls the tool by.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::DockerPs => "docker_ps",
            Self::DockerLogs => "docker_logs",
            Self::DockerInspect => "docker_inspect",
            Self::DockerEvents => "docker_events",
            Self::HostOverview => "host_overview",
            Self::Journal => "journal",
            Self::ListeningPorts => "listening_ports",
            Self::SmartReport => "smart_report",
            Self::IncidentHistory => "incident_history",
            Self::NotesSearch => "notes_search",
            Self::RunReadSkill => "run_read_skill",
        }
    }

    /// What the tool does, as shown to the model.
    #[must_use]
    pub const fn description(&self) -> &'static str {
        match self {
            Self::DockerPs => "List containers on the incident host with state and health.",
            Self::DockerLogs => "Show the last lines of one container's logs.",
            Self::DockerInspect => "Show one container's configuration and state.",
            Self::DockerEvents => "Show recent Docker events, optionally for one container.",
            Self::HostOverview => "Summarise CPU, memory, disks and uptime of the incident host.",
            Self::Journal => "Read recent systemd journal lines, optionally for one unit.",
            Self::ListeningPorts => "List listening TCP and UDP sockets on the incident host.",
            Self::SmartReport => "Show SMART health attributes for one disk.",
            Self::IncidentHistory => "List past incidents with the same fingerprint.",
            Self::NotesSearch => "Search runbook notes for this host and global notes.",
            Self::RunReadSkill => {
                "Run a read-only skill on the incident host and return its output."
            }
        }
    }

    /// Looks up a tool by its [`name`](Self::name).
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tool| tool.name() == name)
    }
}
