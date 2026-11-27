//! What the hub does with a chat gateway's connection state: one log line per change, a
//! finding the owner sees while the gateway is not connected, and a note on the last settings
//! apply when the gateway failed right after it.
//!
//! The Discord shard reconnects by itself after a transport error, pausing longer each time,
//! so the hub does not restart it; it only reports. A close that is final (the token is
//! refused, an intent is not enabled) ends the gateway, and retrying that would only hammer
//! Discord with a token it already refused, so the hub says what to fix instead.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domovoi_core::ports::{AdvisoryRepo, Clock, GatewayStatus, Redactor, SettingsRepo, StoreError};
use domovoi_core::{Advisory, AdvisoryKind};
use domovoi_engine::advisories::advisory_id;
use serde_json::{Value, json};

use crate::settings::stored::LAST_APPLY;

/// Where a gateway stands, as last reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Connected,
    Retrying,
    Down,
}

/// Records gateway state changes for the hub.
pub(crate) struct GatewayHealth {
    advisories: Arc<dyn AdvisoryRepo>,
    settings: Arc<dyn SettingsRepo>,
    clock: Arc<dyn Clock>,
    redactor: Arc<dyn Redactor>,
    states: Mutex<BTreeMap<String, State>>,
}

impl GatewayHealth {
    /// A recorder that writes findings to `advisories` and apply notes to `settings`.
    pub(crate) fn new(
        advisories: Arc<dyn AdvisoryRepo>,
        settings: Arc<dyn SettingsRepo>,
        clock: Arc<dyn Clock>,
        redactor: Arc<dyn Redactor>,
    ) -> Self {
        Self {
            advisories,
            settings,
            clock,
            redactor,
            states: Mutex::new(BTreeMap::new()),
        }
    }

    /// Sets the state of `platform` and says whether it changed.
    fn change(&self, platform: &str, state: State) -> bool {
        let mut states = self
            .states
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        states.insert(platform.to_owned(), state) != Some(state)
    }

    /// The gateway ended for good: raise the finding and note it on the last settings apply.
    pub(crate) async fn down(&self, platform: &str, reason: &str) {
        if !self.change(platform, State::Down) {
            return;
        }
        let reason = self.redactor.redact(reason);
        self.raise(
            platform,
            format!("{} is disconnected", title_case(platform)),
            format!(
                "{reason}. Buttons, slash commands and thread replies on {platform} are off until this is fixed and the hub is restarted (saving settings restarts it)."
            ),
        )
        .await;
        self.note_on_last_apply(&format!(
            "the {platform} gateway failed to start and is not connected: {reason}"
        ))
        .await;
    }

    async fn raise(&self, platform: &str, title: String, detail: String) {
        let id = advisory_id(AdvisoryKind::ChatGatewayDown, None, platform);
        let created_at = self
            .advisories
            .list_open()
            .await
            .ok()
            .and_then(|open| open.into_iter().find(|a| a.id == id).map(|a| a.created_at))
            .unwrap_or_else(|| self.clock.now());
        let advisory = Advisory {
            id,
            kind: AdvisoryKind::ChatGatewayDown,
            host: None,
            title,
            detail,
            created_at,
            resolved_at: None,
        };
        if let Err(err) = self.advisories.upsert(&advisory).await {
            tracing::warn!(%err, platform, "could not record the chat gateway finding");
        }
    }

    /// Adds `warning` to the stored result of the last settings apply, when that apply
    /// succeeded: the settings were applied, and the gateway they configure did not start.
    async fn note_on_last_apply(&self, warning: &str) {
        let Ok(Some(mut record)) = self.settings.get(LAST_APPLY).await else {
            return;
        };
        if record.get("ok") != Some(&Value::Bool(true)) {
            return;
        }
        let Some(object) = record.as_object_mut() else {
            return;
        };
        let warnings = object.entry("warnings").or_insert_with(|| json!([]));
        if let Some(list) = warnings.as_array_mut()
            && !list.iter().any(|w| w == warning)
        {
            list.push(Value::String(warning.to_owned()));
        }
        if let Err(err) = self.settings.set(LAST_APPLY, &record).await {
            tracing::warn!(%err, "could not note the gateway failure on the last apply");
        }
    }
}

#[async_trait]
impl GatewayStatus for GatewayHealth {
    async fn connected(&self, platform: &str) {
        if !self.change(platform, State::Connected) {
            return;
        }
        tracing::info!(platform, "chat gateway connected");
        let id = advisory_id(AdvisoryKind::ChatGatewayDown, None, platform);
        match self.advisories.resolve(id, self.clock.now()).await {
            Ok(()) | Err(StoreError::NotFound) => {}
            Err(err) => tracing::warn!(%err, platform, "could not clear the chat gateway finding"),
        }
    }

    async fn trouble(&self, platform: &str, reason: &str) {
        if !self.change(platform, State::Retrying) {
            return;
        }
        let reason = self.redactor.redact(reason);
        tracing::warn!(
            platform,
            %reason,
            "chat gateway is not connected; it retries by itself with a growing pause, and nothing needs doing unless this lasts"
        );
        self.raise(
            platform,
            format!("{} is not connected", title_case(platform)),
            format!(
                "{reason}. The hub keeps retrying with a growing pause. Buttons, slash commands and thread replies on {platform} are off until it connects."
            ),
        )
        .await;
    }
}

fn title_case(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
