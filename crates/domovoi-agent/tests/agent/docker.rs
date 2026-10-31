//! A fake container source for tool tests.

use async_trait::async_trait;
use domovoi_checks::containers::{ContainerError, ContainerEvent, ContainerInfo, ContainerSource};
use jiff::Timestamp;

/// A container source whose logs the test sets; `hang` makes every call wait forever.
#[derive(Default)]
pub struct FakeDocker {
    pub logs: String,
    pub hang: bool,
}

#[async_trait]
impl ContainerSource for FakeDocker {
    async fn list(&self) -> Result<Vec<ContainerInfo>, ContainerError> {
        Ok(Vec::new())
    }

    async fn events_since(
        &self,
        _since: Timestamp,
        _until: Timestamp,
    ) -> Result<Vec<ContainerEvent>, ContainerError> {
        Ok(Vec::new())
    }

    async fn inspect(&self, name: &str) -> Result<serde_json::Value, ContainerError> {
        Ok(serde_json::json!({ "Name": name }))
    }

    async fn logs(&self, _name: &str, _lines: u16) -> Result<String, ContainerError> {
        if self.hang {
            std::future::pending::<()>().await;
        }
        Ok(self.logs.clone())
    }
}
