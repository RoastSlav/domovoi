//! Recording doubles for the ports next to the fake agent link.

use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;
use domovoi_core::HostId;
use domovoi_core::Signal;
use domovoi_core::ports::{HostReconciler, IngestError, SignalSink};

use crate::lock;
/// A [`SignalSink`] that records every accepted batch.
#[derive(Debug, Default)]
pub struct RecordingSignalSink {
    batches: Mutex<Vec<Vec<Signal>>>,
    failures: Mutex<VecDeque<IngestError>>,
}

impl RecordingSignalSink {
    /// An empty sink.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Makes the next submit fail with `error`; the batch is not recorded.
    pub fn fail_next(&self, error: IngestError) {
        lock(&self.failures).push_back(error);
    }

    /// Every accepted batch, oldest first.
    #[must_use]
    pub fn batches(&self) -> Vec<Vec<Signal>> {
        lock(&self.batches).clone()
    }
}

#[async_trait]
impl SignalSink for RecordingSignalSink {
    async fn submit(&self, signals: Vec<Signal>) -> Result<(), IngestError> {
        if let Some(error) = lock(&self.failures).pop_front() {
            return Err(error);
        }
        lock(&self.batches).push(signals);
        Ok(())
    }
}

/// A [`HostReconciler`] that records every host it was asked to reconcile.
#[derive(Debug, Default)]
pub struct RecordingReconciler {
    hosts: Mutex<Vec<HostId>>,
    notify: tokio::sync::Notify,
}

impl RecordingReconciler {
    /// A reconciler that has been asked about nothing yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Every host it was asked to reconcile, oldest first.
    #[must_use]
    pub fn hosts(&self) -> Vec<HostId> {
        lock(&self.hosts).clone()
    }

    /// Waits until at least `count` hosts have been recorded and returns the list, or
    /// returns at once when that many already were. No polling and no sleeping: it uses
    /// the `Notify` woken by the next recorded host.
    pub async fn wait_for(&self, count: usize) -> Vec<HostId> {
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let hosts = lock(&self.hosts).clone();
            if hosts.len() >= count {
                return hosts;
            }
            notified.await;
        }
    }
}

#[async_trait]
impl HostReconciler for RecordingReconciler {
    async fn reconcile_host(&self, host: HostId) {
        lock(&self.hosts).push(host);
        self.notify.notify_waiters();
    }
}
