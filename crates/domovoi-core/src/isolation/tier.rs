//! Platform tiers: how far a host's isolation checks go, decided from a self-test report.

use serde::{Deserialize, Serialize};

use super::check::CheckId;
use super::report::SelftestReport;

/// How far a platform is trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformTier {
    /// Release-tested and the gate passes.
    Supported,
    /// A systemd host that is not listed but passes every required check.
    Untested,
    /// A required check failed or could not run, or there is no systemd.
    Blocked,
}

impl PlatformTier {
    /// The stable spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::Untested => "untested",
            Self::Blocked => "blocked",
        }
    }
}

/// Whether the helper's recorded platform tier exists and matches the running kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformRecordState {
    /// Present and for this kernel.
    Ok,
    /// Not written yet.
    Missing,
    /// Written for another kernel.
    Stale,
}

/// An operating system release and architecture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformId {
    /// `ID` from `/etc/os-release`.
    pub os_id: String,
    /// `VERSION_ID` from `/etc/os-release`.
    pub version_id: String,
    /// The CPU architecture (`std::env::consts::ARCH`).
    pub arch: String,
}

/// The release-tested platforms. Empty: a platform is promoted only after a real-installer pass.
pub const SUPPORTED_PLATFORMS: &[PlatformId] = &[];

/// The checks that must pass for a host to be anything but blocked. `nd` never satisfies one.
pub const REQUIRED_CHECKS: &[CheckId] = &[
    CheckId::St01,
    CheckId::St02,
    CheckId::St03,
    CheckId::St04,
    CheckId::St05,
    CheckId::St06,
    CheckId::St07,
    CheckId::St09,
    CheckId::St10,
    CheckId::St11,
    CheckId::St12,
    CheckId::St13,
    CheckId::St14,
    CheckId::St15,
    CheckId::St16,
    CheckId::St18,
];

/// The checks that must pass for helper read operations, whatever the tier.
const HELPER_READ_CHECKS: &[CheckId] =
    &[CheckId::St10, CheckId::St11, CheckId::St12, CheckId::St15];

/// The owner's per-host switch `[platform] allow_write_groups_on_untested`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteOptIn(pub bool);

/// Whether skills may run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillsDecision {
    /// Allowed.
    Allow,
    /// Allowed after the owner acknowledges the notice.
    AllowWithNotice,
    /// Refused.
    Refuse,
}

/// Whether helper read groups may be served.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadDecision {
    /// Allowed.
    Allow,
    /// Refused.
    Refuse,
}

/// Whether helper write groups may be served.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteDecision {
    /// Allowed once each group's own gates pass.
    Allow,
    /// Refused until the owner sets the per-host opt-in.
    NeedsOptIn,
    /// Refused.
    Refuse,
}

/// A notice the owner must be shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TierNotice {
    /// The platform is not release-tested.
    UntestedPlatform,
}

impl TierNotice {
    /// The wording shown to the owner.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::UntestedPlatform => {
                "This platform has not been release-tested. Skills run here after you acknowledge that; privileged write operations stay off unless you enable them for this host in helper.toml."
            }
        }
    }
}

/// What a host's tier allows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TierDecision {
    /// The tier.
    pub tier: PlatformTier,
    /// Whether skills may run.
    pub skills: SkillsDecision,
    /// Whether helper read groups may be served.
    pub helper_read: ReadDecision,
    /// Whether helper write groups may be served.
    pub helper_write: WriteDecision,
    /// The notice to show, if any.
    pub notice: Option<TierNotice>,
    /// Required checks that failed or did not run.
    pub blocking: Vec<CheckId>,
    /// The host has no systemd, which blocks it on its own.
    pub no_systemd: bool,
}

/// Decides the tier of the host `report` describes.
///
/// A required check that failed, did not run (`nd`) or is absent blocks the host, as does a host
/// with no systemd. Otherwise a platform in `supported` is Supported and any other Untested. A
/// warning never changes the tier. Helper reads depend only on the helper's own checks.
#[must_use]
pub fn classify_platform(
    report: &SelftestReport,
    supported: &[PlatformId],
    opt_in: WriteOptIn,
) -> TierDecision {
    let satisfied = |id: CheckId| {
        report
            .result(id)
            .is_some_and(|r| r.outcome.satisfies_required())
    };
    let blocking: Vec<CheckId> = REQUIRED_CHECKS
        .iter()
        .copied()
        .filter(|id| !satisfied(*id))
        .collect();
    let no_systemd = report.systemd_version.is_none();
    let tier = if !blocking.is_empty() || no_systemd {
        PlatformTier::Blocked
    } else if supported.contains(&report.platform) {
        PlatformTier::Supported
    } else {
        PlatformTier::Untested
    };
    let (skills, helper_write, notice) = match tier {
        PlatformTier::Supported => (SkillsDecision::Allow, WriteDecision::Allow, None),
        PlatformTier::Untested => (
            SkillsDecision::AllowWithNotice,
            if opt_in.0 {
                WriteDecision::Allow
            } else {
                WriteDecision::NeedsOptIn
            },
            Some(TierNotice::UntestedPlatform),
        ),
        PlatformTier::Blocked => (SkillsDecision::Refuse, WriteDecision::Refuse, None),
    };
    let helper_read = if HELPER_READ_CHECKS.iter().all(|id| satisfied(*id)) {
        ReadDecision::Allow
    } else {
        ReadDecision::Refuse
    };
    TierDecision {
        tier,
        skills,
        helper_read,
        helper_write,
        notice,
        blocking,
        no_systemd,
    }
}

/// The `format` value of a [`PlatformRecord`] this build writes and understands.
pub const PLATFORM_RECORD_FORMAT: u32 = 1;

/// What `selftest --installer --record` writes to the helper's `platform.json`: the tier the
/// last installer self-test decided, tied to the kernel it ran on.
///
/// The helper reads it to answer `Groups`; it never trusts a tier a caller claims.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformRecord {
    /// The record layout, [`PLATFORM_RECORD_FORMAT`].
    pub format: u32,
    /// When the self-test ran, from the injected clock.
    pub recorded_at: jiff::Timestamp,
    /// The kernel release the self-test ran on; a different running kernel makes the record stale.
    pub kernel_release: String,
    /// The operating system release and architecture that was classified.
    pub platform: PlatformId,
    /// The tier decided.
    pub tier: PlatformTier,
    /// The required checks that failed or did not run.
    pub blocking: Vec<CheckId>,
}
