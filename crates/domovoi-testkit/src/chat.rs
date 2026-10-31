//! A chat gateway and a notifier that record what they were asked to do.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domovoi_core::ports::{
    ChatError, ChatGateway, InteractionSink, MessageRef, Notifier, OutboundMessage, ThreadRef,
};
use domovoi_core::{Card, Interaction, InteractionReply};

use crate::lock;

/// One call made to a [`RecordingGateway`].
#[derive(Debug, Clone, PartialEq)]
pub enum GatewayCall {
    /// `post_card`.
    PostCard(Card),
    /// `update_card`.
    UpdateCard(MessageRef, Card),
    /// `open_thread`.
    OpenThread(MessageRef, String),
    /// `reply`.
    Reply(ThreadRef, String),
}

#[derive(Default)]
struct GatewayState {
    calls: Vec<GatewayCall>,
    failures: VecDeque<ChatError>,
    threads: BTreeMap<String, ThreadRef>,
    next_id: u64,
    sink: Option<Arc<dyn InteractionSink>>,
}

/// A [`ChatGateway`] that records every call and lets tests inject interactions.
///
/// Message ids are `msg-1`, `msg-2`, … and thread ids `thread-1`, … in call order.
/// [`run`](ChatGateway::run) stores the sink and returns at once;
/// [`inject`](Self::inject) then delivers to it.
pub struct RecordingGateway {
    platform: String,
    state: Mutex<GatewayState>,
}

impl RecordingGateway {
    /// A gateway for `platform`.
    #[must_use]
    pub fn new(platform: impl Into<String>) -> Self {
        Self {
            platform: platform.into(),
            state: Mutex::new(GatewayState::default()),
        }
    }

    /// Makes the next card, thread or reply call fail with `error`. Failed calls are still
    /// recorded.
    pub fn fail_next(&self, error: ChatError) {
        lock(&self.state).failures.push_back(error);
    }

    /// Every call so far, oldest first.
    #[must_use]
    pub fn calls(&self) -> Vec<GatewayCall> {
        lock(&self.state).calls.clone()
    }

    /// Delivers `interaction` to the sink given to `run` and returns its reply, or `None`
    /// when `run` has not been called yet.
    pub async fn inject(&self, interaction: Interaction) -> Option<InteractionReply> {
        let sink = lock(&self.state).sink.clone()?;
        Some(sink.deliver(interaction).await)
    }

    fn record(&self, call: GatewayCall) -> Result<u64, ChatError> {
        let mut state = lock(&self.state);
        state.calls.push(call);
        if let Some(error) = state.failures.pop_front() {
            return Err(error);
        }
        state.next_id += 1;
        Ok(state.next_id)
    }
}

#[async_trait]
impl ChatGateway for RecordingGateway {
    fn platform(&self) -> &str {
        &self.platform
    }

    async fn post_card(&self, card: &Card) -> Result<MessageRef, ChatError> {
        let n = self.record(GatewayCall::PostCard(card.clone()))?;
        Ok(MessageRef {
            platform: self.platform.clone(),
            id: format!("msg-{n}"),
        })
    }

    async fn update_card(&self, msg: &MessageRef, card: &Card) -> Result<(), ChatError> {
        self.record(GatewayCall::UpdateCard(msg.clone(), card.clone()))
            .map(|_| ())
    }

    async fn open_thread(&self, msg: &MessageRef, title: &str) -> Result<ThreadRef, ChatError> {
        let n = self.record(GatewayCall::OpenThread(msg.clone(), title.to_owned()))?;
        let platform = self.platform.clone();
        let mut state = lock(&self.state);
        let thread = state
            .threads
            .entry(msg.id.clone())
            .or_insert_with(|| ThreadRef {
                platform,
                id: format!("thread-{n}"),
            });
        Ok(thread.clone())
    }

    async fn reply(&self, thread: &ThreadRef, text: &str) -> Result<(), ChatError> {
        self.record(GatewayCall::Reply(thread.clone(), text.to_owned()))
            .map(|_| ())
    }

    async fn run(&self, sink: Arc<dyn InteractionSink>) -> Result<(), ChatError> {
        lock(&self.state).sink = Some(sink);
        Ok(())
    }
}

/// A [`Notifier`] that records every message.
#[derive(Debug)]
pub struct RecordingNotifier {
    id: String,
    sent: Mutex<Vec<OutboundMessage>>,
    failures: Mutex<VecDeque<ChatError>>,
}

impl RecordingNotifier {
    /// A notifier with id `id`.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            sent: Mutex::new(Vec::new()),
            failures: Mutex::new(VecDeque::new()),
        }
    }

    /// Makes the next send fail with `error`. Failed messages are not recorded as sent.
    pub fn fail_next(&self, error: ChatError) {
        lock(&self.failures).push_back(error);
    }

    /// Every message sent so far, oldest first.
    #[must_use]
    pub fn sent(&self) -> Vec<OutboundMessage> {
        lock(&self.sent).clone()
    }
}

#[async_trait]
impl Notifier for RecordingNotifier {
    fn id(&self) -> &str {
        &self.id
    }

    async fn send(&self, msg: &OutboundMessage) -> Result<(), ChatError> {
        if let Some(error) = lock(&self.failures).pop_front() {
            return Err(error);
        }
        lock(&self.sent).push(msg.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use domovoi_core::{CardButton, IncidentId, InteractionKind};
    use jiff::Timestamp;

    use super::*;

    struct EchoSink;

    #[async_trait]
    impl InteractionSink for EchoSink {
        async fn deliver(&self, i: Interaction) -> InteractionReply {
            InteractionReply::Accepted {
                message: Some(format!("{} pressed by {}", i.platform, i.platform_user)),
            }
        }
    }

    fn interaction() -> Interaction {
        Interaction {
            platform: "discord".to_owned(),
            platform_user: "42".to_owned(),
            kind: InteractionKind::Button {
                button: CardButton::Ack,
                incident: IncidentId::new(),
            },
            received_at: Timestamp::constant(1_700_000_000, 0),
        }
    }

    #[tokio::test]
    async fn recording_gateway_round_trips_injected_interaction() {
        let gateway = RecordingGateway::new("discord");
        assert_eq!(gateway.inject(interaction()).await, None);

        gateway.run(Arc::new(EchoSink)).await.unwrap();
        assert_eq!(
            gateway.inject(interaction()).await,
            Some(InteractionReply::Accepted {
                message: Some("discord pressed by 42".to_owned())
            })
        );
    }

    #[tokio::test]
    async fn recording_gateway_records_calls_and_reuses_threads() {
        let gateway = RecordingGateway::new("discord");
        let card = Card {
            incident: IncidentId::new(),
            host: domovoi_core::HostName::parse("nas").unwrap(),
            badge: domovoi_core::CardBadge::Severity(domovoi_core::Severity::Warning),
            headline: "disk".to_owned(),
            bullets: Vec::new(),
            next_step: None,
            buttons: Vec::new(),
            details: None,
            resolved: false,
        };
        let msg = gateway.post_card(&card).await.unwrap();
        let first = gateway.open_thread(&msg, "t").await.unwrap();
        let again = gateway.open_thread(&msg, "t").await.unwrap();
        assert_eq!(first, again);
        gateway.fail_next(ChatError::NotFound);
        assert_eq!(gateway.reply(&first, "hi").await, Err(ChatError::NotFound));
        assert_eq!(gateway.calls().len(), 4);
    }

    #[tokio::test]
    async fn recording_notifier_records_sent_messages() {
        let notifier = RecordingNotifier::new("webhook");
        let msg = OutboundMessage {
            title: "Digest".to_owned(),
            markdown: "all quiet".to_owned(),
            severity: None,
        };
        notifier.fail_next(ChatError::Transport("down".to_owned()));
        assert!(notifier.send(&msg).await.is_err());
        notifier.send(&msg).await.unwrap();
        assert_eq!(notifier.sent(), vec![msg]);
    }
}
