//! A stand-in helper for attestation tests: it holds a signing key and renders and signs
//! reports the way the real helper does, over documentation-range addresses and neutral names.
//! The seed protects nothing; never use it outside tests. Builders return `Result` so a typo
//! shows as an error rather than a panic inside testkit.

use std::collections::BTreeMap;
use std::sync::Mutex;

use async_trait::async_trait;
use domovoi_core::HostId;
use domovoi_core::attest::{
    BootId, CompleteReport, ConfigLines, IncompleteReason, IncompleteReport, LatestOutcome,
    Nonce32, PinnedKey, RouteLines, SafetyInput, TrustView, VerifiedState, render_complete,
    render_incomplete, signing_bytes,
};
use domovoi_core::host::KeyFingerprint;
use domovoi_core::ports::{AttestationReply, HelperTrustRead, StoreError};
use ed25519_dalek::{Signer, SigningKey};
use jiff::Timestamp;

/// The boot id every sample report carries.
pub const BOOT: &str = "5f0c1d2e-3a4b-4c5d-8e9f-0a1b2c3d4e5f";

/// A helper with a signing key made from a one-byte seed.
pub struct TestHelper {
    key: SigningKey,
}

/// Why a sample report could not be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("cannot build a sample report: {0}")]
pub struct AttestBuildError(&'static str);

/// The effective safety set of a sample report. `variant` changes `max_active`, and with it
/// the config hash, so two variants are two different configurations.
fn config(variant: u64) -> Result<ConfigLines, AttestBuildError> {
    ConfigLines::effective(&SafetyInput {
        management_cidrs: vec!["192.0.2.0/24".into(), "2001:db8::/32".into()],
        protected_units: vec!["svc-a".into()],
        protected_containers: vec!["ctr-a".into()],
        max_ttl: Some("2h".into()),
        max_active: Some(variant),
    })
    .map_err(|_| AttestBuildError("the sample configuration"))
}

/// The sample route state; `hub_addr` makes the hub's address a covered path.
fn routes(hub_addr: &str) -> RouteLines {
    let mut lines = RouteLines {
        hub_addr: vec![hub_addr.to_owned()],
        ..RouteLines::default()
    };
    lines.normalise();
    lines
}

fn boot() -> Result<BootId, AttestBuildError> {
    BootId::parse(BOOT).map_err(|_| AttestBuildError("the boot id"))
}

/// The sample complete report for `nonce`, at `generation`, of configuration `variant`.
///
/// # Errors
/// Never in practice: the sample values are constants.
pub fn complete_report(
    nonce: Nonce32,
    generation: u64,
    variant: u64,
) -> Result<CompleteReport, AttestBuildError> {
    let config = config(variant)?;
    let routes = routes("192.0.2.10");
    Ok(CompleteReport {
        helper_key_id: "0123456789abcdef".to_owned(),
        nonce,
        boot_id: boot()?,
        generation,
        config_hash: config.hash(),
        route_state_hash: routes.hash(),
        computed_at: Timestamp::constant(1_700_000_000, 0),
        config,
        routes,
    })
}

impl TestHelper {
    /// A helper whose key is the 32 bytes of `seed`.
    #[must_use]
    pub fn new(seed: u8) -> Self {
        Self {
            key: SigningKey::from_bytes(&[seed; 32]),
        }
    }

    /// The 32 raw public-key bytes.
    #[must_use]
    pub fn public_key(&self) -> [u8; 32] {
        self.key.verifying_key().to_bytes()
    }

    /// The fingerprint the hub computes for this key.
    #[must_use]
    pub fn fingerprint(&self) -> KeyFingerprint {
        KeyFingerprint::of_public_key(&self.public_key())
    }

    /// Signs `report` bytes the way the helper does: over the signing prefix and the report.
    #[must_use]
    pub fn sign(&self, report: Vec<u8>) -> AttestationReply {
        AttestationReply::Signed {
            public_key: self.public_key(),
            signature: self.key.sign(&signing_bytes(&report)).to_bytes(),
            report,
        }
    }

    /// A signed complete report for `nonce`.
    ///
    /// # Errors
    /// Never in practice: the sample values are constants.
    pub fn complete(
        &self,
        nonce: Nonce32,
        generation: u64,
        variant: u64,
    ) -> Result<AttestationReply, AttestBuildError> {
        Ok(self.sign(render_complete(&complete_report(
            nonce, generation, variant,
        )?)))
    }

    /// A signed incomplete report for `nonce` with `reason`.
    ///
    /// # Errors
    /// `reason` is not a well-formed token.
    pub fn incomplete(
        &self,
        nonce: Nonce32,
        reason: &str,
    ) -> Result<AttestationReply, AttestBuildError> {
        Ok(self.sign(render_incomplete(&IncompleteReport {
            helper_key_id: "0123456789abcdef".to_owned(),
            nonce,
            boot_id: boot()?,
            reason: IncompleteReason::new(reason).ok_or(AttestBuildError("the reason token"))?,
            computed_at: Timestamp::constant(1_700_000_000, 0),
        })))
    }
}

/// The verified state a hub would hold after accepting the sample report for `fingerprint`,
/// whose nonce was issued at `issued_at`.
///
/// # Errors
/// Never in practice: the sample values are constants.
pub fn verified_state(
    fingerprint: KeyFingerprint,
    generation: u64,
    variant: u64,
    issued_at: Timestamp,
) -> Result<VerifiedState, AttestBuildError> {
    let report = complete_report(Nonce32::from_bytes([1; 32]), generation, variant)?;
    let bytes = render_complete(&report);
    Ok(VerifiedState::from_report(
        &report,
        bytes,
        fingerprint,
        issued_at,
        issued_at,
    ))
}

/// A trust view with `helper`'s key pinned, the latest outcome verified and the sample
/// report's state verified at `issued_at`.
///
/// # Errors
/// Never in practice: the sample values are constants.
pub fn verified_view(
    helper: &TestHelper,
    generation: u64,
    variant: u64,
    issued_at: Timestamp,
) -> Result<TrustView, AttestBuildError> {
    Ok(TrustView {
        pin: Some(PinnedKey::new(helper.public_key(), issued_at, "user:test")),
        received_key: Some(helper.public_key()),
        latest: Some(LatestOutcome::Verified),
        verified: Some(verified_state(
            helper.fingerprint(),
            generation,
            variant,
            issued_at,
        )?),
    })
}

/// A [`HelperTrustRead`] whose views the test sets per host. A host never set is a host never
/// seen: every field empty.
#[derive(Default)]
pub struct FakeHelperTrust {
    views: Mutex<BTreeMap<HostId, TrustView>>,
}

impl FakeHelperTrust {
    /// A source with no views.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets what `host`'s helper looks like from now on.
    pub fn set(&self, host: HostId, view: TrustView) {
        crate::lock(&self.views).insert(host, view);
    }
}

#[async_trait]
impl HelperTrustRead for FakeHelperTrust {
    async fn view(&self, host: HostId) -> Result<TrustView, StoreError> {
        Ok(crate::lock(&self.views)
            .get(&host)
            .cloned()
            .unwrap_or_default())
    }
}
