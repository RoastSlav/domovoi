//! The templates of the built-in reads the agent answers in a skill's place: the last day of
//! the journal, services, timers and processes.

use domovoi_core::ports::ProcessSpec;

use super::{CommandError, Commands, owned, spec};

/// Most lines [`Commands::journal_day`] reads.
pub const SUMMARY_LINES: u16 = 5000;

/// Which of the last day's journal lines [`Commands::journal_day`] reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayPriority {
    /// Warnings and worse.
    Warnings,
    /// Errors and worse.
    Errors,
}

/// Which services [`Commands::systemctl_services`] lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceState {
    /// Running now.
    Running,
    /// Failed.
    Failed,
}

impl Commands {
    /// The newest journal entries of the last 24 hours, at most `lines` and at most
    /// [`SUMMARY_LINES`]: for [`DayPriority::Warnings`] only each entry's program name,
    /// one per line, so that long messages do not use up the output limit; for
    /// [`DayPriority::Errors`] the lines themselves with ISO times.
    pub fn journal_day(
        &self,
        priority: DayPriority,
        lines: u16,
    ) -> Result<ProcessSpec, CommandError> {
        let program = Self::tool(&self.journalctl, "journalctl")?;
        let format: &[&str] = match priority {
            DayPriority::Warnings => &[
                "--output=cat",
                "--output-fields=SYSLOG_IDENTIFIER",
                "--priority=warning",
            ],
            DayPriority::Errors => &["--output=short-iso", "--priority=err"],
        };
        let lines = lines.clamp(1, SUMMARY_LINES);
        let mut args = owned(&["--no-pager", "--since=-24h"]);
        args.extend(owned(format));
        args.push(format!("--lines={lines}"));
        Ok(spec(program, args))
    }

    /// The service units in `state`, one per line.
    pub fn systemctl_services(&self, state: ServiceState) -> Result<ProcessSpec, CommandError> {
        let program = Self::tool(&self.systemctl, "systemctl")?;
        let state = match state {
            ServiceState::Running => "--state=running",
            ServiceState::Failed => "--state=failed",
        };
        Ok(spec(
            program,
            owned(&[
                "list-units",
                "--type=service",
                state,
                "--no-legend",
                "--plain",
                "--no-pager",
            ]),
        ))
    }

    /// The active timers.
    pub fn systemctl_timers(&self) -> Result<ProcessSpec, CommandError> {
        let program = Self::tool(&self.systemctl, "systemctl")?;
        Ok(spec(
            program,
            owned(&["list-timers", "--no-legend", "--no-pager"]),
        ))
    }

    /// Whether this account can see process 1, which a hidden process list would not.
    pub fn ps_first(&self) -> Result<ProcessSpec, CommandError> {
        let program = Self::tool(&self.ps, "ps")?;
        Ok(spec(program, owned(&["-o", "pid=", "-p", "1"])))
    }

    /// Every process, sorted by CPU or by memory use, largest first.
    pub fn ps_by(&self, memory: bool) -> Result<ProcessSpec, CommandError> {
        let program = Self::tool(&self.ps, "ps")?;
        let sort = if memory {
            "--sort=-%mem"
        } else {
            "--sort=-%cpu"
        };
        Ok(spec(
            program,
            owned(&["-eo", "pid,user,%cpu,%mem,rss,etime,comm", sort]),
        ))
    }
}
