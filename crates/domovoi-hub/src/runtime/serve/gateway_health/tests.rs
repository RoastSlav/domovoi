//! A chat gateway that is not connected shows up: a finding, one log line per change, and a
//! note on the last settings apply.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domovoi_core::ports::{
    AdvisoryRepo, ChatError, ChatGateway, Clock, GatewayStatus, InteractionSink, MessageRef,
    SettingsRepo, ThreadRef,
};
use domovoi_core::{AdvisoryKind, Card, Interaction, InteractionReply};
use domovoi_engine::PatternRedactor;
use domovoi_engine::advisories::advisory_id;
use domovoi_testkit::log_capture::LogCapture;
use domovoi_testkit::{FakeClock, InMemoryStore};
use serde_json::json;
use tokio_util::sync::CancellationToken;

use super::GatewayHealth;
use crate::settings::stored::LAST_APPLY;

fn health(store: &Arc<InMemoryStore>) -> Arc<GatewayHealth> {
    let clock: Arc<dyn Clock> = Arc::new(FakeClock::new("2026-11-27T10:00:00Z".parse().unwrap()));
    Arc::new(GatewayHealth::new(
        store.clone(),
        store.clone(),
        clock,
        Arc::new(PatternRedactor::default()),
    ))
}

async fn open_gateway_findings(store: &InMemoryStore) -> Vec<domovoi_core::Advisory> {
    store
        .list_open()
        .await
        .unwrap()
        .into_iter()
        .filter(|a| a.kind == AdvisoryKind::ChatGatewayDown)
        .collect()
}

/// A gateway whose `run` ends at once with an error, as Discord's does when the token is
/// refused.
struct Ending {
    outcome: Mutex<Option<ChatError>>,
}

struct NoSink;

#[async_trait]
impl InteractionSink for NoSink {
    async fn deliver(&self, _i: Interaction) -> InteractionReply {
        InteractionReply::Rejected {
            reason: "not used".to_owned(),
        }
    }
}

#[async_trait]
impl ChatGateway for Ending {
    fn platform(&self) -> &str {
        "discord"
    }

    async fn post_card(&self, _card: &Card) -> Result<MessageRef, ChatError> {
        Err(ChatError::NotFound)
    }

    async fn update_card(&self, _msg: &MessageRef, _card: &Card) -> Result<(), ChatError> {
        Ok(())
    }

    async fn open_thread(&self, _msg: &MessageRef, _title: &str) -> Result<ThreadRef, ChatError> {
        Err(ChatError::NotFound)
    }

    async fn reply(&self, _thread: &ThreadRef, _text: &str) -> Result<(), ChatError> {
        Ok(())
    }

    async fn run(&self, _sink: Arc<dyn InteractionSink>) -> Result<(), ChatError> {
        match self.outcome.lock().unwrap().take() {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }
}

#[tokio::test]
async fn a_gateway_that_fails_to_start_is_a_finding_and_a_note_on_the_apply_result() {
    let (logs, _guard) = LogCapture::start();
    let store = Arc::new(InMemoryStore::new());
    store
        .set(
            LAST_APPLY,
            &json!({ "ok": true, "at": "2026-11-27T10:00:00Z" }),
        )
        .await
        .unwrap();
    let health = health(&store);
    let gateway: Arc<dyn ChatGateway> = Arc::new(Ending {
        outcome: Mutex::new(Some(ChatError::Transport(
            "the Discord gateway closed the connection for good and will not reconnect; check that the bot token is valid".to_owned(),
        ))),
    });
    super::super::run_gateway(
        gateway,
        Arc::new(NoSink),
        CancellationToken::new(),
        health.clone(),
    )
    .await;

    let findings = open_gateway_findings(&store).await;
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].title, "Discord is disconnected");
    assert!(
        findings[0].detail.contains("bot token is valid"),
        "the finding says why: {}",
        findings[0].detail
    );
    assert!(
        findings[0].detail.contains("restarted"),
        "and what to do: {}",
        findings[0].detail
    );

    let apply = store.get(LAST_APPLY).await.unwrap().unwrap();
    assert_eq!(apply["ok"], true, "the settings were applied");
    let warnings = apply["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1, "{apply}");
    assert!(
        warnings[0]
            .as_str()
            .unwrap()
            .contains("discord gateway failed to start"),
        "{apply}"
    );
    assert_eq!(
        logs.lines_with("chat gateway stopped").len(),
        1,
        "{}",
        logs.text()
    );

    // Saying it again changes nothing: one finding, one warning.
    health.down("discord", "the same failure").await;
    assert_eq!(open_gateway_findings(&store).await.len(), 1);
    let apply = store.get(LAST_APPLY).await.unwrap().unwrap();
    assert_eq!(apply["warnings"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn trouble_raises_the_finding_and_connecting_clears_it_each_logged_once() {
    let (logs, _guard) = LogCapture::start();
    let store = Arc::new(InMemoryStore::new());
    let health = health(&store);
    health.trouble("discord", "connection reset").await;
    health.trouble("discord", "connection reset again").await;
    let findings = open_gateway_findings(&store).await;
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].title, "Discord is not connected");
    assert!(findings[0].detail.contains("connection reset"));
    assert_eq!(logs.lines_with("chat gateway is not connected").len(), 1);

    health.connected("discord").await;
    health.connected("discord").await;
    assert!(open_gateway_findings(&store).await.is_empty());
    assert_eq!(logs.lines_with("chat gateway connected").len(), 1);
    let id = advisory_id(AdvisoryKind::ChatGatewayDown, None, "discord");
    assert!(store.list_open().await.unwrap().iter().all(|a| a.id != id));
}

#[tokio::test]
async fn a_rolled_back_apply_record_is_left_alone() {
    let store = Arc::new(InMemoryStore::new());
    let record = json!({ "ok": false, "at": "2026-11-27T10:00:00Z", "message": "went back" });
    store.set(LAST_APPLY, &record).await.unwrap();
    health(&store).down("discord", "closed").await;
    assert_eq!(store.get(LAST_APPLY).await.unwrap().unwrap(), record);
}
