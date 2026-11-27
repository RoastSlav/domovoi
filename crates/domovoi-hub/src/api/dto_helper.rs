//! What the helper endpoints take and return. Every value is computed by the hub from the
//! owner's pin and the verified report; text an agent reported is carried in its own field and
//! marked as unverified. Nothing here carries key bytes, report bytes, a signature or a nonce.

use domovoi_core::attest::UnverifiedReason;
use domovoi_core::ports::HelperLive;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::helper_trust::{AttestationState, KeyState, VerifiedDetail};
use crate::services::HelperView;

/// Why the helper's attestation is not verified: the closed set the wire and the gate use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum UnverifiedReasonDto {
    /// No key is pinned.
    KeyNotPinned,
    /// The helper presents a key other than the pinned one.
    KeyMismatch,
    /// No verified attestation is stored.
    AttestationMissing,
    /// The last verified attestation is too old.
    AttestationStale,
    /// A signature did not verify.
    SignatureInvalid,
    /// The helper signed a report that says it could not compute the set.
    ReportIncomplete,
    /// The generation went backwards or repeated with different contents.
    GenerationRegressed,
    /// The verified set does not cover the path.
    PathUncovered,
    /// A confirmation no longer matches the verified set.
    ConfirmationStale,
}

impl From<UnverifiedReason> for UnverifiedReasonDto {
    fn from(reason: UnverifiedReason) -> Self {
        match reason {
            UnverifiedReason::KeyNotPinned => Self::KeyNotPinned,
            UnverifiedReason::KeyMismatch => Self::KeyMismatch,
            UnverifiedReason::AttestationMissing => Self::AttestationMissing,
            UnverifiedReason::AttestationStale => Self::AttestationStale,
            UnverifiedReason::SignatureInvalid => Self::SignatureInvalid,
            UnverifiedReason::ReportIncomplete => Self::ReportIncomplete,
            UnverifiedReason::GenerationRegressed => Self::GenerationRegressed,
            UnverifiedReason::PathUncovered => Self::PathUncovered,
            UnverifiedReason::ConfirmationStale => Self::ConfirmationStale,
        }
    }
}

/// What the agent said about the helper. Unverified text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct HelperStateDto {
    /// `absent`, `ok` or `error`, as the agent reported.
    pub state: HelperStateWord,
    /// The operations the helper offers, as the agent reported.
    pub ops: Vec<String>,
    /// What the agent said went wrong, when the state is `error`.
    pub reason: Option<String>,
}

/// The agent's word for the helper's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum HelperStateWord {
    /// No helper.
    Absent,
    /// The helper answered.
    Ok,
    /// The helper reported an error.
    Error,
}

/// A key the helper's session relayed, with the fingerprint the hub computed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ReceivedKeyDto {
    /// 64 lowercase hex characters.
    pub fingerprint: String,
    /// The same, in groups of four, as `domovoi helper fingerprint` prints it.
    pub fingerprint_groups: String,
    /// The first 16 characters of the fingerprint: a label, never trusted.
    pub key_id: String,
}

/// The key the owner pinned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PinDto {
    /// 64 lowercase hex characters.
    pub fingerprint: String,
    /// When it was pinned.
    pub pinned_at: String,
    /// Who pinned it.
    pub pinned_by: String,
}

/// Where the key stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum KeyStateDto {
    /// No key from the helper and none pinned.
    NoHelper,
    /// A key is held from the helper and not pinned.
    NotPinned,
    /// Pinned, and the helper presents the same key.
    Pinned,
    /// The helper presents a key other than the pinned one.
    Changed,
}

/// The verdict on the attestation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AttestationStateDto {
    /// Verified and fresh.
    Verified,
    /// Verified, but older than the freshness window.
    Stale,
    /// Not verified.
    Unverified,
}

/// Where the attestation stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct AttestationDto {
    /// The verdict.
    pub state: AttestationStateDto,
    /// Why it is not verified, when it is not.
    pub reason: Option<UnverifiedReasonDto>,
    /// The token of a validly signed incomplete report. Display only.
    pub helper_reason: Option<String>,
    /// The unsigned token an agent relayed. Unverified: the agent can invent it.
    pub unsigned_hint: Option<String>,
    /// When the hub last recorded an outcome or asked.
    pub checked_at: Option<String>,
}

/// The verified effective set. Present only while the attestation is verified and fresh.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct VerifiedDto {
    /// The report generation.
    pub generation: u64,
    /// The boot the report was made in.
    pub boot_id: String,
    /// Hash of the effective configuration.
    pub config_hash: String,
    /// Hash of the observed route state.
    pub route_state_hash: String,
    /// When the hub issued the nonce the report answers.
    pub nonce_issued_at: String,
    /// Seconds since then.
    pub age_seconds: i64,
    /// Whether the report is within the freshness window.
    pub fresh: bool,
    /// When the helper computed the report. Display only.
    pub computed_at: String,
    /// The longest a containment may last, in seconds.
    pub max_ttl_s: u64,
    /// How many containments may be active; zero allows none.
    pub max_active: u64,
    /// The configured management networks.
    pub cidrs: Vec<String>,
    /// The protected unit names.
    pub protected_units: Vec<String>,
    /// The protected container names.
    pub protected_containers: Vec<String>,
    /// The units the helper may restart (`[systemd] restart_units` in `helper.toml`).
    pub restart_units: Vec<String>,
    /// The hub addresses the helper found.
    pub hub_addrs: Vec<String>,
    /// The link peers the helper found, as `address:port`.
    pub link_peers: Vec<String>,
}

/// What the hub shows about a host's helper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct HelperDto {
    /// The host's name.
    pub host: String,
    /// What the agent said about the helper. Unverified.
    pub helper: HelperStateDto,
    /// The key the helper's session relayed.
    pub received_key: Option<ReceivedKeyDto>,
    /// The owner's pin.
    pub pin: Option<PinDto>,
    /// Where the key stands.
    pub key_state: KeyStateDto,
    /// Where the attestation stands.
    pub attestation: AttestationDto,
    /// The verified effective set, only while the attestation passes.
    pub verified: Option<VerifiedDto>,
}

/// The owner's request to pin the key the hub holds from the helper.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct PinKeyRequest {
    /// The full fingerprint printed by `domovoi helper fingerprint` on the host; spaces and
    /// case are ignored.
    pub fingerprint: String,
    /// The account password, asked for again.
    #[serde(default)]
    pub password: Option<String>,
}

/// The owner's request to unpin the helper key.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct UnpinKeyRequest {
    /// The account password, asked for again.
    #[serde(default)]
    pub password: Option<String>,
}

fn groups(fingerprint: &str) -> String {
    fingerprint
        .as_bytes()
        .chunks(4)
        .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

fn verified(detail: &VerifiedDetail) -> VerifiedDto {
    let state = &detail.state;
    VerifiedDto {
        generation: state.identity.generation,
        boot_id: state.identity.boot_id.as_str().to_owned(),
        config_hash: state.identity.config_hash.to_string(),
        route_state_hash: state.identity.route_state_hash.to_string(),
        nonce_issued_at: state.nonce_issued_at.to_string(),
        age_seconds: detail.age_seconds,
        fresh: true,
        computed_at: detail.computed_at.to_string(),
        max_ttl_s: state.max_ttl_s,
        max_active: state.max_active,
        cidrs: state
            .cidrs
            .iter()
            .map(domovoi_core::attest::Cidr::render)
            .collect(),
        protected_units: detail.protected_units.clone(),
        protected_containers: detail.protected_containers.clone(),
        restart_units: detail.restart_units.clone(),
        hub_addrs: state.hub_addrs.iter().map(ToString::to_string).collect(),
        link_peers: state
            .link_peers
            .iter()
            .map(|(addr, port)| match addr {
                core::net::IpAddr::V4(_) => format!("{addr}:{port}"),
                core::net::IpAddr::V6(_) => format!("[{addr}]:{port}"),
            })
            .collect(),
    }
}

impl From<&HelperView> for HelperDto {
    fn from(view: &HelperView) -> Self {
        let detail = &view.detail;
        let helper = match detail.helper.as_ref().map(|report| &report.live) {
            None | Some(HelperLive::Absent) => HelperStateDto {
                state: HelperStateWord::Absent,
                ops: Vec::new(),
                reason: None,
            },
            Some(HelperLive::Ok { ops }) => HelperStateDto {
                state: HelperStateWord::Ok,
                ops: ops.clone(),
                reason: None,
            },
            Some(HelperLive::Error { reason }) => HelperStateDto {
                state: HelperStateWord::Error,
                ops: Vec::new(),
                reason: Some(reason.clone()),
            },
        };
        Self {
            host: view.host.to_string(),
            helper,
            received_key: detail.received_key.as_ref().map(|key| {
                let fingerprint = key.fingerprint.to_string();
                ReceivedKeyDto {
                    fingerprint_groups: groups(&fingerprint),
                    fingerprint,
                    key_id: key.key_id.clone(),
                }
            }),
            pin: detail.pin.as_ref().map(|pin| PinDto {
                fingerprint: pin.fingerprint.to_string(),
                pinned_at: pin.pinned_at.to_string(),
                pinned_by: pin.pinned_by.clone(),
            }),
            key_state: match detail.key_state {
                KeyState::NoHelper => KeyStateDto::NoHelper,
                KeyState::NotPinned => KeyStateDto::NotPinned,
                KeyState::Pinned => KeyStateDto::Pinned,
                KeyState::Changed => KeyStateDto::Changed,
            },
            attestation: AttestationDto {
                state: match detail.attestation.state {
                    AttestationState::Verified => AttestationStateDto::Verified,
                    AttestationState::Stale => AttestationStateDto::Stale,
                    AttestationState::Unverified => AttestationStateDto::Unverified,
                },
                reason: detail.attestation.reason.map(UnverifiedReasonDto::from),
                helper_reason: detail.attestation.helper_reason.clone(),
                unsigned_hint: detail.attestation.unsigned_hint.clone(),
                checked_at: detail.attestation.checked_at.map(|at| at.to_string()),
            },
            verified: detail.verified.as_ref().map(verified),
        }
    }
}
