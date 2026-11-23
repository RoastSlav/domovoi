//! Check ids, outcomes and results of the self-test.

use serde::{Deserialize, Serialize};

use crate::identity::{CheckToken, PathFault};

macro_rules! check_ids {
    ($($variant:ident => $text:literal, $run_by:ident;)+) => {
        /// Identifies one check, `ST-01` to `ST-20`.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        pub enum CheckId {
            $(#[doc = $text] #[serde(rename = $text)] $variant,)+
        }

        impl CheckId {
            /// All twenty checks in order.
            pub const ALL: &'static [Self] = &[$(Self::$variant,)+];

            /// The spelling `ST-01` to `ST-20`.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text,)+ }
            }

            /// Which party normally collects this check.
            #[must_use]
            pub const fn run_by(self) -> RunBy {
                match self { $(Self::$variant => RunBy::$run_by,)+ }
            }
        }
    };
}

check_ids! {
    St01 => "ST-01", Agent;
    St02 => "ST-02", Agent;
    St03 => "ST-03", Root;
    St04 => "ST-04", Root;
    St05 => "ST-05", Runner;
    St06 => "ST-06", Runner;
    St07 => "ST-07", Root;
    St08 => "ST-08", Root;
    St09 => "ST-09", Root;
    St10 => "ST-10", Root;
    St11 => "ST-11", Root;
    St12 => "ST-12", Agent;
    St13 => "ST-13", Runner;
    St14 => "ST-14", Agent;
    St15 => "ST-15", Agent;
    St16 => "ST-16", Agent;
    St17 => "ST-17", Root;
    St18 => "ST-18", Root;
    St19 => "ST-19", Root;
    St20 => "ST-20", Root;
}

impl std::fmt::Display for CheckId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Who runs a check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunBy {
    /// The unprivileged control agent.
    Agent,
    /// Root, from the installer or `selftest --installer`.
    Root,
    /// The skill runner in probe mode.
    Runner,
}

/// How a check ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Outcome {
    /// The check passed.
    Pass,
    /// The check failed.
    Fail {
        /// Why, in the closed token set.
        token: CheckToken,
        /// What was found, for the operator.
        detail: String,
    },
    /// Something is off but nothing is unsafe; never changes the tier.
    Warn {
        /// Why.
        token: CheckToken,
        /// What was found.
        detail: String,
    },
    /// A fact recorded, compared with nothing.
    Info {
        /// The fact.
        detail: String,
    },
    /// Not run here. Never a pass, and never satisfies a required check.
    Nd {
        /// Why it did not run.
        reason: String,
    },
}

impl Outcome {
    /// A failure.
    #[must_use]
    pub fn fail(token: CheckToken, detail: impl Into<String>) -> Self {
        Self::Fail {
            token,
            detail: detail.into(),
        }
    }

    /// A warning.
    #[must_use]
    pub fn warn(token: CheckToken, detail: impl Into<String>) -> Self {
        Self::Warn {
            token,
            detail: detail.into(),
        }
    }

    /// A recorded fact.
    #[must_use]
    pub fn info(detail: impl Into<String>) -> Self {
        Self::Info {
            detail: detail.into(),
        }
    }

    /// A check that did not run.
    #[must_use]
    pub fn nd(reason: impl Into<String>) -> Self {
        Self::Nd {
            reason: reason.into(),
        }
    }

    /// Whether this is a failure.
    #[must_use]
    pub const fn is_fail(&self) -> bool {
        matches!(self, Self::Fail { .. })
    }

    /// Whether this is `nd`.
    #[must_use]
    pub const fn is_nd(&self) -> bool {
        matches!(self, Self::Nd { .. })
    }

    /// Whether a required check counts as satisfied: a pass, or a warning (which never blocks).
    #[must_use]
    pub const fn satisfies_required(&self) -> bool {
        matches!(self, Self::Pass | Self::Warn { .. })
    }
}

/// The result of one check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckResult {
    /// Which check.
    pub id: CheckId,
    /// Who collected it.
    pub run_by: RunBy,
    /// How it ended.
    pub outcome: Outcome,
    /// Every offending path, with found and expected values; empty for most checks.
    #[serde(default)]
    pub faults: Vec<PathFault>,
}

impl CheckResult {
    /// A result with no path faults, collected by the check's usual party.
    #[must_use]
    pub fn new(id: CheckId, outcome: Outcome) -> Self {
        Self {
            id,
            run_by: id.run_by(),
            outcome,
            faults: Vec::new(),
        }
    }

    /// The same result carrying `faults`.
    #[must_use]
    pub fn with_faults(mut self, faults: Vec<PathFault>) -> Self {
        self.faults = faults;
        self
    }

    /// The same result collected by `run_by`.
    #[must_use]
    pub const fn by(mut self, run_by: RunBy) -> Self {
        self.run_by = run_by;
        self
    }
}
