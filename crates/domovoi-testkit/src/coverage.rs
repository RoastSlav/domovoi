//! A scriptable [`ManagementCoverage`] for tests.

use std::collections::BTreeSet;
use std::sync::Mutex;

use async_trait::async_trait;
use domovoi_core::HostId;
use domovoi_core::knowledge::{CoverageEvidence, CoverageVerdict, UnverifiableReason};
use domovoi_core::ports::ManagementCoverage;
use jiff::Timestamp;

use crate::lock;

#[derive(Clone)]
enum Mode {
    Unavailable,
    Unverifiable(UnverifiableReason),
    Covering(BTreeSet<String>),
}

/// How `confirmation_current` answers.
type CurrencyAnswer = Result<(), UnverifiableReason>;

struct Inner {
    mode: Mode,
    calls: Vec<(HostId, String)>,
    currency: Option<CurrencyAnswer>,
}

/// A coverage check whose answers the test chooses. The mode can change at run time.
pub struct FakeCoverage {
    inner: Mutex<Inner>,
}

impl FakeCoverage {
    fn with(mode: Mode) -> Self {
        Self {
            inner: Mutex::new(Inner {
                mode,
                calls: Vec::new(),
                currency: None,
            }),
        }
    }

    /// A check that is not available: every verdict is `Unverifiable { NotAvailable }`.
    #[must_use]
    pub fn unavailable() -> Self {
        Self::with(Mode::Unavailable)
    }

    /// A check that answers `Covered` for the listed ranges and `Uncovered` for all others.
    #[must_use]
    pub fn covering(cidrs: &[&str]) -> Self {
        let fake = Self::with(Mode::Unavailable);
        fake.cover(cidrs);
        fake
    }

    /// Switches to answering `Covered` (evidence `fake-evidence`) for `cidrs`.
    pub fn cover(&self, cidrs: &[&str]) {
        lock(&self.inner).mode = Mode::Covering(cidrs.iter().map(|c| (*c).to_owned()).collect());
    }

    /// Switches to answering `Unverifiable` with `reason`.
    pub fn set_unverifiable(&self, reason: UnverifiableReason) {
        lock(&self.inner).mode = Mode::Unverifiable(reason);
    }

    /// Switches back to the unavailable mode.
    pub fn make_unavailable(&self) {
        lock(&self.inner).mode = Mode::Unavailable;
    }

    /// Makes `confirmation_current` answer `answer`. Without this it fails closed with
    /// `NotAvailable`, as the trait's default does.
    pub fn set_currency(&self, answer: Result<(), UnverifiableReason>) {
        lock(&self.inner).currency = Some(answer);
    }

    /// The `(host, cidr)` pairs asked about so far.
    #[must_use]
    pub fn calls(&self) -> Vec<(HostId, String)> {
        lock(&self.inner).calls.clone()
    }
}

#[async_trait]
impl ManagementCoverage for FakeCoverage {
    fn is_available(&self) -> bool {
        !matches!(lock(&self.inner).mode, Mode::Unavailable)
    }

    async fn verdict(&self, host: HostId, cidr: &str) -> CoverageVerdict {
        let mut inner = lock(&self.inner);
        inner.calls.push((host, cidr.to_owned()));
        match &inner.mode {
            Mode::Unavailable => CoverageVerdict::Unverifiable {
                reason: UnverifiableReason::NotAvailable,
            },
            Mode::Unverifiable(reason) => CoverageVerdict::Unverifiable { reason: *reason },
            Mode::Covering(set) if set.contains(cidr) => {
                match CoverageEvidence::new("fake-evidence") {
                    Some(evidence) => CoverageVerdict::Covered { evidence },
                    None => CoverageVerdict::Uncovered,
                }
            }
            Mode::Covering(_) => CoverageVerdict::Uncovered,
        }
    }

    async fn confirmation_current(
        &self,
        _host: HostId,
        _evidence: &CoverageEvidence,
        _confirmed_at: Timestamp,
    ) -> Result<(), UnverifiableReason> {
        lock(&self.inner)
            .currency
            .unwrap_or(Err(UnverifiableReason::NotAvailable))
    }
}
