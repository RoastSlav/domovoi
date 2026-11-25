//! A container-engine read port that answers from a script and records every call.

use std::sync::Mutex;

use async_trait::async_trait;
use domovoi_core::helper::read::{ContainerRef, ContainerWire, DockerEngineInfo, EventWire};
use domovoi_core::ports::{DockerReadApi, DockerReadError};
use jiff::Timestamp;

use crate::lock;

/// One call made on a [`FakeDockerReadApi`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DockerReadCall {
    /// `engine()`.
    Engine,
    /// `list()`.
    List,
    /// `events(since, until)`.
    Events(Timestamp, Timestamp),
    /// `inspect(container)`.
    Inspect(String),
    /// `logs(container, lines)`.
    Logs(String, u16),
}

/// What a [`FakeDockerReadApi`] answers with.
#[derive(Debug, Clone)]
pub struct DockerReadScript {
    /// The engine probe answer.
    pub engine: Result<DockerEngineInfo, DockerReadError>,
    /// The list answer.
    pub list: Result<Vec<ContainerWire>, DockerReadError>,
    /// The events answer.
    pub events: Result<Vec<EventWire>, DockerReadError>,
    /// The inspect answer, whatever the container.
    pub inspect: Result<serde_json::Value, DockerReadError>,
    /// The logs answer, whatever the container.
    pub logs: Result<String, DockerReadError>,
}

impl Default for DockerReadScript {
    fn default() -> Self {
        Self {
            engine: Err(DockerReadError::Absent),
            list: Ok(Vec::new()),
            events: Ok(Vec::new()),
            inspect: Err(DockerReadError::NotFound),
            logs: Err(DockerReadError::NotFound),
        }
    }
}

/// A scripted [`DockerReadApi`] that records each call.
#[derive(Debug, Default)]
pub struct FakeDockerReadApi {
    script: Mutex<DockerReadScript>,
    calls: Mutex<Vec<DockerReadCall>>,
}

impl FakeDockerReadApi {
    /// A fake that answers with `script`.
    #[must_use]
    pub fn new(script: DockerReadScript) -> Self {
        Self {
            script: Mutex::new(script),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// Replaces the script.
    pub fn set(&self, script: DockerReadScript) {
        *lock(&self.script) = script;
    }

    /// Every call so far, in order.
    #[must_use]
    pub fn calls(&self) -> Vec<DockerReadCall> {
        lock(&self.calls).clone()
    }

    fn record(&self, call: DockerReadCall) {
        lock(&self.calls).push(call);
    }
}

#[async_trait]
impl DockerReadApi for FakeDockerReadApi {
    async fn engine(&self) -> Result<DockerEngineInfo, DockerReadError> {
        self.record(DockerReadCall::Engine);
        lock(&self.script).engine.clone()
    }

    async fn list(&self) -> Result<Vec<ContainerWire>, DockerReadError> {
        self.record(DockerReadCall::List);
        lock(&self.script).list.clone()
    }

    async fn events(
        &self,
        since: Timestamp,
        until: Timestamp,
    ) -> Result<Vec<EventWire>, DockerReadError> {
        self.record(DockerReadCall::Events(since, until));
        lock(&self.script).events.clone()
    }

    async fn inspect(
        &self,
        container: &ContainerRef,
    ) -> Result<serde_json::Value, DockerReadError> {
        self.record(DockerReadCall::Inspect(container.as_str().to_owned()));
        lock(&self.script).inspect.clone()
    }

    async fn logs(&self, container: &ContainerRef, lines: u16) -> Result<String, DockerReadError> {
        self.record(DockerReadCall::Logs(container.as_str().to_owned(), lines));
        lock(&self.script).logs.clone()
    }
}
