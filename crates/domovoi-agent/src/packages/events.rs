//! Audit events for package operations, with a tracing sink for production.
//!
//! Events carry names, versions, key ids and digests, never archive contents.

use domovoi_core::package::{
    ArchiveDigest, KeyId, PackageFailureCode, PackageName, PackageVersion,
};
use uuid::Uuid;

/// What happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    /// Signature, trust and digest checks passed.
    Verified,
    /// An install was refused.
    Rejected,
    /// A package was installed.
    Installed,
    /// A package was removed.
    Uninstalled,
    /// A registered package's signer is not trusted now (reported at start).
    KeyUntrusted,
    /// Recovery changed the files on disk at start.
    Recovered,
    /// A transfer was refused or failed.
    TransferRejected,
}

impl EventKind {
    /// The fixed event name, for example `package.verified`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "package.verified",
            Self::Rejected => "package.rejected",
            Self::Installed => "package.installed",
            Self::Uninstalled => "package.uninstalled",
            Self::KeyUntrusted => "package.key_untrusted",
            Self::Recovered => "package.recovered",
            Self::TransferRejected => "package.transfer_rejected",
        }
    }
}

/// One audit event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageEvent {
    /// What happened.
    pub kind: EventKind,
    /// The execution it belongs to, when it has one.
    pub execution_id: Option<Uuid>,
    /// The package, when known.
    pub package: Option<PackageName>,
    /// The version, when known.
    pub version: Option<PackageVersion>,
    /// The signing key, when known.
    pub key_id: Option<KeyId>,
    /// The archive digest, when known.
    pub digest: Option<ArchiveDigest>,
    /// The failure code, for rejections.
    pub code: Option<PackageFailureCode>,
    /// Extra plain text (already escaped), for recovery notes and rejections.
    pub detail: Option<String>,
}

impl PackageEvent {
    /// An event of `kind` with nothing else set.
    #[must_use]
    pub fn new(kind: EventKind) -> Self {
        Self {
            kind,
            execution_id: None,
            package: None,
            version: None,
            key_id: None,
            digest: None,
            code: None,
            detail: None,
        }
    }

    /// Sets the execution id.
    #[must_use]
    pub fn execution(mut self, id: Uuid) -> Self {
        self.execution_id = Some(id);
        self
    }

    /// Sets the package and version.
    #[must_use]
    pub fn package(mut self, name: &PackageName, version: &PackageVersion) -> Self {
        self.package = Some(name.clone());
        self.version = Some(version.clone());
        self
    }

    /// Sets the signing key.
    #[must_use]
    pub fn key(mut self, key_id: &KeyId) -> Self {
        self.key_id = Some(key_id.clone());
        self
    }

    /// Sets the archive digest.
    #[must_use]
    pub fn digest(mut self, digest: &ArchiveDigest) -> Self {
        self.digest = Some(*digest);
        self
    }

    /// Sets the failure code.
    #[must_use]
    pub fn code(mut self, code: PackageFailureCode) -> Self {
        self.code = Some(code);
        self
    }

    /// Sets the detail text.
    #[must_use]
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

/// Where audit events go.
pub trait EventSink: Send + Sync {
    /// Records `event`; must not block.
    fn emit(&self, event: PackageEvent);
}

/// Writes events to `tracing` under the target `domovoi::package_audit`.
#[derive(Debug, Default, Clone, Copy)]
pub struct TracingEvents;

impl EventSink for TracingEvents {
    fn emit(&self, event: PackageEvent) {
        let text = |value: Option<String>| value.unwrap_or_default();
        tracing::info!(
            target: "domovoi::package_audit",
            event = event.kind.as_str(),
            execution_id = %text(event.execution_id.map(|id| id.to_string())),
            package = %text(event.package.map(|value| value.to_string())),
            version = %text(event.version.map(|value| value.to_string())),
            key_id = %text(event.key_id.map(|value| value.to_string())),
            archive_sha256 = %text(event.digest.map(|value| value.to_string())),
            code = %text(event.code.map(|value| value.to_string())),
            detail = %text(event.detail),
        );
    }
}

/// A sink that keeps what it receives, for tests.
#[cfg(test)]
#[derive(Debug, Default)]
pub struct RecordingEvents {
    events: std::sync::Mutex<Vec<PackageEvent>>,
}

#[cfg(test)]
impl RecordingEvents {
    /// Everything emitted so far.
    #[must_use]
    pub fn events(&self) -> Vec<PackageEvent> {
        self.events
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default()
    }
}

#[cfg(test)]
impl EventSink for RecordingEvents {
    fn emit(&self, event: PackageEvent) {
        if let Ok(mut events) = self.events.lock() {
            events.push(event);
        }
    }
}
