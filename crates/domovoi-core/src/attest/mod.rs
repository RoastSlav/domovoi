//! The helper safety attestation contract: the line-based report, its two domain-separated
//! hashes, the signing input, the safety identity, the latest-outcome gate and the pure
//! decision functions built on them.
//!
//! Everything here is pure. The helper process renders and signs reports with this code, the
//! hub parses and verifies them with it, and the engine decides coverage from the typed values
//! it returns. Signing, key files, route observation and TOML parsing live elsewhere.
//!
//! A value is accepted only when it is already canonical: every typed line parses strictly and
//! re-renders to the identical text, so the bytes that were signed are the bytes that were
//! checked.

mod boot;
mod config;
mod decide;
mod expected;
mod field;
pub mod gate;
mod hash;
mod identity;
mod lines;
mod parse;
mod reason;
mod report;
mod scalar;
mod selector;
mod view;

use jiff::SignedDuration;

pub use boot::{BootId, BootIdError, Nonce32};
pub use config::{
    ConfigError, ConfigLines, MANDATORY_UNITS, SafetyInput, canonical_unit, check_container,
    parse_max_ttl,
};
pub use decide::{
    Authority, Decision, PolicyRequiresHuman, agent_accepts, decide, unprotected_paths,
};
pub use expected::{ExpectedSafety, MalformedSafety, require_expected_safety};
pub use field::{EncodeError, decode_field, encode_field, is_literal};
pub use gate::{confirmation_current, covered, gate, permit_identity_current};
pub use hash::{Hash32, HashError, ZERO_HASH, domain_bytes, domain_hash, signing_bytes};
pub use identity::{IdentityError, IdentityMatch, SafetyIdentity, compare};
pub use lines::{EndpointRoute, LinkPeer, LocalAddr, Route, Rule};
pub use parse::{check_line, parse_report};
pub use reason::{IncompleteReason, UnverifiedReason, UnverifiedReasonError};
pub use report::{
    CompleteReport, IncompleteReport, Report, ReportError, RouteLines, render_complete,
    render_incomplete,
};
pub use scalar::{Cidr, Family, check_ifname, parse_ip, parse_port, parse_u32, parse_uint};
pub use selector::Selector;
pub use view::{LatestOutcome, PinnedKey, TrustView, VerifiedState};

/// The report format number this build writes and the only one it reads.
pub const REPORT_FORMAT: u32 = 1;
/// The domain prefix of the signing input: the prefix, then the report bytes.
pub const SIGNING_PREFIX: &str = "domovoi-safety-attest-v1\n";
/// The domain prefix of the config hash input.
pub const CONFIG_PREFIX: &str = "domovoi-safety-config-v1\n";
/// The domain prefix of the route-state hash input.
pub const ROUTES_PREFIX: &str = "domovoi-safety-routes-v1\n";
/// The most bytes a report may have. Equal to the wire's base64 decode cap, which a protocol
/// test asserts.
pub const MAX_REPORT_BYTES: usize = 64 * 1024;
/// How long an issued nonce stays usable. Strict: an age of this much or more is expired.
pub const NONCE_TTL: SignedDuration = SignedDuration::from_secs(60);
/// How long a verified attestation counts as fresh, measured from nonce issue time. Strict:
/// an age of this much or more is stale.
pub const FRESHNESS: SignedDuration = SignedDuration::from_secs(300);
/// How long an owner's confirmation of a management path stays current.
pub const CONFIRMATION_MAX_AGE: SignedDuration = SignedDuration::from_secs(30 * 86_400);
