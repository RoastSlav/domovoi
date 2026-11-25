//! Why a unit cannot be bound or restarted: one token per trigger, with a plain sentence.

use std::fmt;

/// The closed reason behind `target_unsupported`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnsupportedReason {
    /// The name failed the grammar.
    BadName,
    /// The unit is masked (load state not `loaded`).
    Masked,
    /// The unit is transient.
    Transient,
    /// The unit file is linked.
    Linked,
    /// The unit is generated.
    Generated,
    /// The unit is enabled only for this boot (`enabled-runtime`).
    EnabledRuntime,
    /// The unit file state is an alias.
    Alias,
    /// The unit is a template instance. The helper never parses an instance name, so it cannot
    /// enumerate the template's drop-in directory; instances are refused.
    Instance,
    /// Any other unit file state.
    UnitFileState,
    /// A path holds a control character.
    ControlCharacter,
    /// A file lives outside `/etc/systemd/system` and `/usr/lib/systemd/system`.
    Location(String),
    /// A file is a symbolic link.
    Symlink(String),
}

impl UnsupportedReason {
    /// The stable reason word.
    #[must_use]
    pub fn word(&self) -> &'static str {
        match self {
            Self::BadName => "bad_name",
            Self::Masked => "masked",
            Self::Transient => "transient",
            Self::Linked => "linked",
            Self::Generated => "generated",
            Self::EnabledRuntime => "enabled_runtime",
            Self::Alias => "alias",
            Self::Instance => "instance",
            Self::UnitFileState => "unit_file_state",
            Self::ControlCharacter => "control_character",
            Self::Location(_) => "location",
            Self::Symlink(_) => "symlink",
        }
    }
}

/// Cuts a path to at most 200 characters for a refusal.
fn short(path: &str) -> String {
    path.chars().take(200).collect()
}

/// Why a unit was refused, with the token the helper, hub and card all use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnitRefusal {
    /// The unit is of a class never supported.
    TargetUnsupported(UnsupportedReason),
    /// The manager has no such unit.
    TargetNotFound,
    /// The unit is in the protected set.
    TargetProtected,
    /// The unit is outside the helper allowlist.
    TargetNotAllowlisted,
    /// systemd has changes on disk it has not loaded.
    TargetStale,
    /// The unit changed since it was bound; names the first differing field.
    TargetChanged {
        /// The first differing field.
        field: String,
    },
    /// Something could not be read; nothing is known.
    ReadFailed,
    /// The host's platform is untested and the per-host opt-in is off.
    PlatformUntested,
    /// The host's platform is blocked.
    PlatformBlocked,
    /// The helper-side restart budget is spent.
    RestartBudgetExhausted,
    /// `systemctl restart` failed.
    RestartFailed,
    /// The unit hit its start limit.
    StartLimitHit,
    /// The invocation did not change after a zero exit.
    NoRestartObserved,
    /// The new process is not running what was approved; names the fields.
    WrongStart {
        /// The differing field names, never values.
        fields: Vec<String>,
    },
}

impl UnitRefusal {
    /// Builds a `location` refusal naming `path`.
    #[must_use]
    pub fn location(path: &str) -> Self {
        Self::TargetUnsupported(UnsupportedReason::Location(short(path)))
    }

    /// Builds a `symlink` refusal naming `path`.
    #[must_use]
    pub fn symlink(path: &str) -> Self {
        Self::TargetUnsupported(UnsupportedReason::Symlink(short(path)))
    }

    /// The stable token.
    #[must_use]
    pub fn token(&self) -> &'static str {
        match self {
            Self::TargetUnsupported(_) => "target_unsupported",
            Self::TargetNotFound => "target_not_found",
            Self::TargetProtected => "target_protected",
            Self::TargetNotAllowlisted => "target_not_allowlisted",
            Self::TargetStale => "target_stale",
            Self::TargetChanged { .. } => "target_changed",
            Self::ReadFailed => "read_failed",
            Self::PlatformUntested => "platform_untested",
            Self::PlatformBlocked => "platform_blocked",
            Self::RestartBudgetExhausted => "restart_budget_exhausted",
            Self::RestartFailed => "restart_failed",
            Self::StartLimitHit => "start_limit_hit",
            Self::NoRestartObserved => "no_restart_observed",
            Self::WrongStart { .. } => "wrong_start",
        }
    }

    /// The detail carried after the token, if any.
    #[must_use]
    pub fn detail(&self) -> Option<String> {
        match self {
            Self::TargetUnsupported(reason) => Some(match reason {
                UnsupportedReason::Location(p) | UnsupportedReason::Symlink(p) => {
                    format!("{}: {p}", reason.word())
                }
                other => other.word().to_owned(),
            }),
            Self::TargetChanged { field } => Some(field.clone()),
            Self::WrongStart { fields } => Some(fields.join(",")),
            _ => None,
        }
    }

    /// A plain sentence for the card and for error text. Never holds file contents or
    /// environment values.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::TargetUnsupported(reason) => match reason {
                UnsupportedReason::BadName => "That is not a valid service name.".to_owned(),
                UnsupportedReason::Masked => "The unit is masked.".to_owned(),
                UnsupportedReason::Transient => "The unit is transient.".to_owned(),
                UnsupportedReason::Linked => "The unit file is linked.".to_owned(),
                UnsupportedReason::Generated => "The unit is generated.".to_owned(),
                UnsupportedReason::EnabledRuntime => {
                    "The unit is enabled for this boot only.".to_owned()
                }
                UnsupportedReason::Alias => "The unit is an alias.".to_owned(),
                UnsupportedReason::Instance => {
                    "Template instances are not supported: their drop-in directory cannot be checked."
                        .to_owned()
                }
                UnsupportedReason::UnitFileState => {
                    "The unit's file state is not supported.".to_owned()
                }
                UnsupportedReason::ControlCharacter => {
                    "A unit file path holds a control character.".to_owned()
                }
                UnsupportedReason::Location(p) => {
                    format!("{p} is outside /etc/systemd/system and /usr/lib/systemd/system.")
                }
                UnsupportedReason::Symlink(p) => format!("{p} is a symbolic link."),
            },
            Self::TargetNotFound => "systemd does not know that unit.".to_owned(),
            Self::TargetProtected => "That unit is protected and is never restarted.".to_owned(),
            Self::TargetNotAllowlisted => {
                "That unit is not on the helper's restart list.".to_owned()
            }
            Self::TargetStale => "systemd has changes for this unit on disk that it has not \
                                  loaded. Run `systemctl daemon-reload` on the host, then ask \
                                  again. Domovoi never reloads for you."
                .to_owned(),
            Self::TargetChanged { field } => {
                format!("The unit changed since it was approved ({field}). Approve it again.")
            }
            Self::ReadFailed => {
                "The unit could not be read, so nothing is known about it.".to_owned()
            }
            Self::PlatformUntested => "This platform is not release-tested; restarts are off \
                                       until you switch them on for this host."
                .to_owned(),
            Self::PlatformBlocked => "This platform is blocked for restarts.".to_owned(),
            Self::RestartBudgetExhausted => {
                "Too many restarts recently; the helper refused another.".to_owned()
            }
            Self::RestartFailed => "systemd could not restart the unit.".to_owned(),
            Self::StartLimitHit => "The service is down: the start limit was hit.".to_owned(),
            Self::NoRestartObserved => "systemd did not restart the unit.".to_owned(),
            Self::WrongStart { .. } => {
                "The service may not be running what you approved.".to_owned()
            }
        }
    }

    /// Parses a stored `"<token>: <detail>"` message back into a refusal, when the token is
    /// one of these. `None` for anything else.
    #[must_use]
    pub fn from_message(message: &str) -> Option<Self> {
        let (token, detail) = match message.split_once(": ") {
            Some((t, d)) => (t, Some(d)),
            None => (message, None),
        };
        Some(match token {
            "target_not_found" => Self::TargetNotFound,
            "target_protected" => Self::TargetProtected,
            "target_not_allowlisted" => Self::TargetNotAllowlisted,
            "target_stale" => Self::TargetStale,
            "target_changed" => Self::TargetChanged {
                field: detail.unwrap_or_default().to_owned(),
            },
            "read_failed" => Self::ReadFailed,
            "platform_untested" => Self::PlatformUntested,
            "platform_blocked" => Self::PlatformBlocked,
            "restart_budget_exhausted" => Self::RestartBudgetExhausted,
            "restart_failed" => Self::RestartFailed,
            "start_limit_hit" => Self::StartLimitHit,
            "no_restart_observed" => Self::NoRestartObserved,
            "wrong_start" => Self::WrongStart {
                fields: detail
                    .unwrap_or_default()
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect(),
            },
            "target_unsupported" => Self::TargetUnsupported(match detail.unwrap_or_default() {
                "bad_name" => UnsupportedReason::BadName,
                "masked" => UnsupportedReason::Masked,
                "transient" => UnsupportedReason::Transient,
                "linked" => UnsupportedReason::Linked,
                "generated" => UnsupportedReason::Generated,
                "enabled_runtime" => UnsupportedReason::EnabledRuntime,
                "alias" => UnsupportedReason::Alias,
                "instance" => UnsupportedReason::Instance,
                "control_character" => UnsupportedReason::ControlCharacter,
                d if d.starts_with("location: ") => {
                    UnsupportedReason::Location(d["location: ".len()..].to_owned())
                }
                d if d.starts_with("symlink: ") => {
                    UnsupportedReason::Symlink(d["symlink: ".len()..].to_owned())
                }
                _ => UnsupportedReason::UnitFileState,
            }),
            _ => return None,
        })
    }

    /// Whether the helper proves nothing restarted when it answers with this refusal.
    #[must_use]
    pub fn is_pre_restart(&self) -> bool {
        !matches!(
            self,
            Self::RestartFailed
                | Self::StartLimitHit
                | Self::NoRestartObserved
                | Self::WrongStart { .. }
        )
    }
}

impl fmt::Display for UnitRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.detail() {
            Some(detail) => write!(f, "{}: {detail}", self.token()),
            None => f.write_str(self.token()),
        }
    }
}
