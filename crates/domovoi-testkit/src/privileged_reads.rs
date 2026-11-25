//! A privileged-read port that answers from a script and records every call.

use std::sync::Mutex;

use async_trait::async_trait;
use domovoi_core::helper::read::{DeviceRef, KernelLogOutput, ListenerWire, SmartReportOutput};
use domovoi_core::ports::{PrivilegedError, PrivilegedReads};

use crate::lock;

/// One call made on a [`FakePrivilegedReads`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrivilegedCall {
    /// `smart_scan()`.
    SmartScan,
    /// `smart_report(device)`.
    SmartReport(String),
    /// `kernel_log(since_minutes, lines)`.
    KernelLog(u32, u16),
    /// `net_listeners()`.
    NetListeners,
}

/// What a [`FakePrivilegedReads`] answers with.
#[derive(Debug, Clone)]
pub struct PrivilegedScript {
    /// The scan answer.
    pub scan: Result<Vec<DeviceRef>, PrivilegedError>,
    /// The report answer, whatever the device.
    pub report: Result<SmartReportOutput, PrivilegedError>,
    /// The kernel log answer.
    pub kernel: Result<KernelLogOutput, PrivilegedError>,
    /// The listeners answer.
    pub listeners: Result<Vec<ListenerWire>, PrivilegedError>,
}

impl Default for PrivilegedScript {
    fn default() -> Self {
        Self {
            scan: Ok(Vec::new()),
            report: Err(PrivilegedError::HelperMissing),
            kernel: Err(PrivilegedError::HelperMissing),
            listeners: Ok(Vec::new()),
        }
    }
}

/// A scripted [`PrivilegedReads`] that records each call.
#[derive(Debug, Default)]
pub struct FakePrivilegedReads {
    script: Mutex<PrivilegedScript>,
    calls: Mutex<Vec<PrivilegedCall>>,
}

impl FakePrivilegedReads {
    /// A fake that answers with `script`.
    #[must_use]
    pub fn new(script: PrivilegedScript) -> Self {
        Self {
            script: Mutex::new(script),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// Replaces the script.
    pub fn set(&self, script: PrivilegedScript) {
        *lock(&self.script) = script;
    }

    /// Every call so far, in order.
    #[must_use]
    pub fn calls(&self) -> Vec<PrivilegedCall> {
        lock(&self.calls).clone()
    }

    fn record(&self, call: PrivilegedCall) {
        lock(&self.calls).push(call);
    }
}

#[async_trait]
impl PrivilegedReads for FakePrivilegedReads {
    async fn smart_scan(&self) -> Result<Vec<DeviceRef>, PrivilegedError> {
        self.record(PrivilegedCall::SmartScan);
        lock(&self.script).scan.clone()
    }

    async fn smart_report(&self, device: &DeviceRef) -> Result<SmartReportOutput, PrivilegedError> {
        self.record(PrivilegedCall::SmartReport(device.as_str().to_owned()));
        lock(&self.script).report.clone()
    }

    async fn kernel_log(
        &self,
        since_minutes: u32,
        lines: u16,
    ) -> Result<KernelLogOutput, PrivilegedError> {
        self.record(PrivilegedCall::KernelLog(since_minutes, lines));
        lock(&self.script).kernel.clone()
    }

    async fn net_listeners(&self) -> Result<Vec<ListenerWire>, PrivilegedError> {
        self.record(PrivilegedCall::NetListeners);
        lock(&self.script).listeners.clone()
    }
}
