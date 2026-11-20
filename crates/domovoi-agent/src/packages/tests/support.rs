//! Helpers shared by the package tests.

use std::sync::Mutex;

use async_trait::async_trait;
use domovoi_config::PackageLimits;
use domovoi_core::package::{PackageFailureCode, PackageName};
use domovoi_core::ports::Clock;
use domovoi_testkit::{BuiltPackage, FakeClock};
use jiff::{SignedDuration, Timestamp};

use crate::packages::archive::{Audit, audit};
use crate::packages::error::PackageError;

/// A fixed instant for fake clocks.
pub(super) fn start() -> Timestamp {
    "2026-11-18T10:00:00Z".parse().unwrap()
}

/// A clock that moves forward `step` every time it is read.
#[derive(Debug)]
pub(crate) struct SteppingClock {
    now: Mutex<Timestamp>,
    step: SignedDuration,
}

impl SteppingClock {
    pub(crate) fn new(step: SignedDuration) -> Self {
        Self {
            now: Mutex::new(start()),
            step,
        }
    }
}

#[async_trait]
impl Clock for SteppingClock {
    fn now(&self) -> Timestamp {
        let Ok(mut now) = self.now.lock() else {
            return start();
        };
        let current = *now;
        if let Ok(next) = current.checked_add(self.step) {
            *now = next;
        }
        current
    }

    async fn sleep_until(&self, _t: Timestamp) {}
}

/// The package name used by most fixtures.
pub(super) fn name() -> PackageName {
    PackageName::parse("pkg-a").unwrap()
}

/// Audits `built` for `pkg-a` with `limits` and the real digest on a frozen clock.
pub(super) fn audit_with(
    built: &BuiltPackage,
    limits: &PackageLimits,
) -> Result<Audit, PackageError> {
    let clock = FakeClock::new(start());
    audit(
        built.archive.as_slice(),
        &name(),
        limits,
        &clock,
        &built.archive_sha256,
    )
}

/// The failure code of `result`, or a panic naming what succeeded.
pub(super) fn code_of<T: std::fmt::Debug>(result: Result<T, PackageError>) -> PackageFailureCode {
    match result {
        Err(error) => error.code(),
        Ok(value) => panic!("expected a failure, got {value:?}"),
    }
}
