//! The closed set of commands checks and read tools may run.
//!
//! Each template takes typed, validated slots and returns a [`ProcessSpec`] with an
//! absolute program path, a fixed argv, a timeout and an output cap. Every slot is its own
//! argv element (or the value part of a single `--flag=value` element), and no slot may
//! start with `-`, so a slot can never become an option or a shell word. Program paths come
//! from fixed candidate lists checked with [`HostFs::exists`]; there is no `PATH` lookup.

use std::collections::BTreeMap;
use std::fmt;

use domovoi_core::ports::{HostFs, ProcessSpec};
use jiff::SignedDuration;

/// Largest output any template captures, per stream.
pub const MAX_OUTPUT_BYTES: u32 = 2 * 1024 * 1024;
/// How long any template may run.
pub const TIMEOUT: SignedDuration = SignedDuration::from_secs(15);
/// Most lines [`Commands::journal`] returns.
pub const MAX_JOURNAL_LINES: u16 = 500;
/// Furthest back [`Commands::journal`] looks, in minutes.
pub const MAX_JOURNAL_MINUTES: u32 = 1440;
/// How far back [`Commands::journal_since_cursor`] looks when there is no cursor yet.
pub const FIRST_RUN_MINUTES: u32 = 15;

const JOURNALCTL: &[&str] = &["/usr/bin/journalctl", "/bin/journalctl"];
const SMARTCTL: &[&str] = &["/usr/sbin/smartctl", "/usr/bin/smartctl", "/sbin/smartctl"];
const SYSTEMCTL: &[&str] = &["/usr/bin/systemctl", "/bin/systemctl"];
const FAIL2BAN: &[&str] = &["/usr/bin/fail2ban-client", "/usr/local/bin/fail2ban-client"];

/// Why a template could not produce a command.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommandError {
    /// None of the tool's candidate paths exists on this host.
    #[error("{0} is not installed")]
    NotInstalled(&'static str),
    /// A slot value failed validation.
    #[error("invalid {slot}: {reason}")]
    InvalidSlot {
        /// Which slot.
        slot: &'static str,
        /// What is wrong.
        reason: String,
    },
}

fn invalid(slot: &'static str, reason: impl Into<String>) -> CommandError {
    CommandError::InvalidSlot {
        slot,
        reason: reason.into(),
    }
}

fn check_chars(
    slot: &'static str,
    input: &str,
    max: usize,
    allowed: impl Fn(u8) -> bool,
) -> Result<(), CommandError> {
    if input.is_empty() || input.len() > max {
        return Err(invalid(slot, format!("must be 1 to {max} characters")));
    }
    if input.starts_with('-') {
        return Err(invalid(slot, "must not start with '-'"));
    }
    if !input.bytes().all(allowed) {
        return Err(invalid(slot, "contains a character that is not allowed"));
    }
    Ok(())
}

macro_rules! slot {
    ($(#[$doc:meta])* $name:ident, $slot:literal, $max:literal, $allowed:expr) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// Validates `input`.
            pub fn parse(input: &str) -> Result<Self, CommandError> {
                check_chars($slot, input, $max, $allowed)?;
                Ok(Self(input.to_owned()))
            }

            /// The value.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

slot!(
    /// A systemd unit name: `[A-Za-z0-9@._:-]{1,128}`, not starting with `-`.
    UnitName,
    "unit name",
    128,
    |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'@' | b'.' | b'_' | b':' | b'-')
);
slot!(
    /// A fail2ban jail name: `[A-Za-z0-9_-]{1,64}`, not starting with `-`.
    JailName,
    "jail name",
    64,
    |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')
);
slot!(
    /// A systemd property name such as `ActiveState`: ASCII letters only.
    UnitProperty,
    "unit property",
    64,
    |b: u8| b.is_ascii_alphabetic()
);
slot!(
    /// A journal cursor: printable ASCII without spaces, at most 512 characters.
    JournalCursor,
    "journal cursor",
    512,
    |b: u8| b.is_ascii_graphic()
);

slot!(
    /// A syslog identifier such as `sudo`: `[A-Za-z0-9_.-]{1,64}`, not starting with `-`.
    SyslogIdentifier,
    "syslog identifier",
    64,
    |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-')
);

/// Which journal messages [`Commands::journal_since_cursor`] reads.
///
/// journalctl combines filters of different kinds with AND, so one call reads one kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalScope<'a> {
    /// The kernel ring buffer.
    Kernel,
    /// Messages from any of these units.
    Units(&'a [UnitName]),
    /// Messages with any of these syslog identifiers.
    Identifiers(&'a [SyslogIdentifier]),
}

/// A disk device path: `/dev/` followed by `[a-z0-9]+`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DevicePath(String);

impl DevicePath {
    /// Validates `input`.
    pub fn parse(input: &str) -> Result<Self, CommandError> {
        let name = input
            .strip_prefix("/dev/")
            .ok_or_else(|| invalid("device path", "must start with /dev/"))?;
        let ok = !name.is_empty()
            && name.len() <= 32
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
        if !ok {
            return Err(invalid(
                "device path",
                "must be /dev/ followed by a-z and 0-9",
            ));
        }
        Ok(Self(input.to_owned()))
    }

    /// The path.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The resolved program paths of the tools the templates use.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Commands {
    journalctl: Option<String>,
    smartctl: Option<String>,
    systemctl: Option<String>,
    fail2ban_client: Option<String>,
}

async fn first_existing(fs: &dyn HostFs, candidates: &[&str]) -> Option<String> {
    for candidate in candidates {
        if fs.exists(candidate).await {
            return Some((*candidate).to_owned());
        }
    }
    None
}

fn spec(program: &str, args: Vec<String>) -> ProcessSpec {
    ProcessSpec {
        program: program.to_owned(),
        args,
        env: BTreeMap::new(),
        cwd: None,
        stdin: None,
        timeout: TIMEOUT,
        max_output_bytes: MAX_OUTPUT_BYTES,
        run_as: None,
    }
}

fn owned(args: &[&str]) -> Vec<String> {
    args.iter().map(|a| (*a).to_owned()).collect()
}

impl Commands {
    /// Looks up each tool once in its fixed candidate list.
    pub async fn resolve(fs: &dyn HostFs) -> Self {
        Self {
            journalctl: first_existing(fs, JOURNALCTL).await,
            smartctl: first_existing(fs, SMARTCTL).await,
            systemctl: first_existing(fs, SYSTEMCTL).await,
            fail2ban_client: first_existing(fs, FAIL2BAN).await,
        }
    }

    fn tool<'a>(path: &'a Option<String>, name: &'static str) -> Result<&'a str, CommandError> {
        path.as_deref().ok_or(CommandError::NotInstalled(name))
    }

    /// The last `lines` journal lines from the last `since_minutes`, for one unit or the
    /// kernel ring.
    pub fn journal(
        &self,
        unit: Option<&UnitName>,
        since_minutes: u32,
        lines: u16,
        kernel: bool,
    ) -> Result<ProcessSpec, CommandError> {
        let program = Self::tool(&self.journalctl, "journalctl")?;
        if !(1..=MAX_JOURNAL_MINUTES).contains(&since_minutes) {
            return Err(invalid(
                "since_minutes",
                format!("must be 1 to {MAX_JOURNAL_MINUTES}"),
            ));
        }
        if !(1..=MAX_JOURNAL_LINES).contains(&lines) {
            return Err(invalid(
                "lines",
                format!("must be 1 to {MAX_JOURNAL_LINES}"),
            ));
        }
        let mut args = owned(&["--no-pager", "--quiet", "--output=short-iso"]);
        args.push(format!("--since=-{since_minutes}min"));
        args.push(format!("--lines={lines}"));
        if kernel {
            args.push("--dmesg".to_owned());
        }
        if let Some(unit) = unit {
            args.push("--unit".to_owned());
            args.push(unit.to_string());
        }
        Ok(spec(program, args))
    }

    /// Journal messages in `scope` after `cursor` (or from the last [`FIRST_RUN_MINUTES`]
    /// without one), message text only, ending with a `-- cursor: ...` line.
    pub fn journal_since_cursor(
        &self,
        cursor: Option<&JournalCursor>,
        scope: JournalScope<'_>,
    ) -> Result<ProcessSpec, CommandError> {
        let program = Self::tool(&self.journalctl, "journalctl")?;
        let mut args = owned(&["--no-pager", "--quiet", "--output=cat", "--show-cursor"]);
        match cursor {
            Some(cursor) => args.push(format!("--after-cursor={cursor}")),
            None => args.push(format!("--since=-{FIRST_RUN_MINUTES}min")),
        }
        match scope {
            JournalScope::Kernel => args.push("--dmesg".to_owned()),
            JournalScope::Units(units) => {
                if units.is_empty() {
                    return Err(invalid("unit name", "at least one unit is needed"));
                }
                for unit in units {
                    args.push("--unit".to_owned());
                    args.push(unit.to_string());
                }
            }
            JournalScope::Identifiers(ids) => {
                if ids.is_empty() {
                    return Err(invalid(
                        "syslog identifier",
                        "at least one identifier is needed",
                    ));
                }
                for id in ids {
                    args.push("--identifier".to_owned());
                    args.push(id.to_string());
                }
            }
        }
        Ok(spec(program, args))
    }

    /// Every disk smartctl can open, as JSON.
    pub fn smartctl_scan(&self) -> Result<ProcessSpec, CommandError> {
        let program = Self::tool(&self.smartctl, "smartctl")?;
        Ok(spec(program, owned(&["--scan-open", "--json"])))
    }

    /// Full SMART report for one device, as JSON, without waking a sleeping disk.
    pub fn smartctl_all(&self, device: &DevicePath) -> Result<ProcessSpec, CommandError> {
        let program = Self::tool(&self.smartctl, "smartctl")?;
        let mut args = owned(&["--all", "--json", "--nocheck=standby"]);
        args.push(device.as_str().to_owned());
        Ok(spec(program, args))
    }

    /// Selected properties of one unit, as `Key=Value` lines.
    pub fn systemctl_show(
        &self,
        unit: &UnitName,
        props: &[UnitProperty],
    ) -> Result<ProcessSpec, CommandError> {
        let program = Self::tool(&self.systemctl, "systemctl")?;
        if props.is_empty() {
            return Err(invalid("unit property", "at least one property is needed"));
        }
        let list: Vec<&str> = props.iter().map(UnitProperty::as_str).collect();
        let mut args = owned(&["show", "--no-pager"]);
        args.push(format!("--property={}", list.join(",")));
        args.push("--".to_owned());
        args.push(unit.to_string());
        Ok(spec(program, args))
    }

    /// Every failed unit, one per line.
    pub fn systemctl_failed(&self) -> Result<ProcessSpec, CommandError> {
        let program = Self::tool(&self.systemctl, "systemctl")?;
        Ok(spec(
            program,
            owned(&[
                "list-units",
                "--state=failed",
                "--no-legend",
                "--plain",
                "--no-pager",
            ]),
        ))
    }

    /// Every failed unit as a JSON array (systemd 246 and later).
    pub fn systemctl_failed_json(&self) -> Result<ProcessSpec, CommandError> {
        let program = Self::tool(&self.systemctl, "systemctl")?;
        Ok(spec(
            program,
            owned(&[
                "list-units",
                "--state=failed",
                "--no-legend",
                "--plain",
                "--no-pager",
                "--output=json",
            ]),
        ))
    }

    /// Status of one fail2ban jail.
    pub fn fail2ban_status(&self, jail: &JailName) -> Result<ProcessSpec, CommandError> {
        let program = Self::tool(&self.fail2ban_client, "fail2ban-client")?;
        Ok(spec(program, vec!["status".to_owned(), jail.to_string()]))
    }
}
