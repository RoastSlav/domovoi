//! `domovoi notify test`: one test card per chat gateway, one test message per notifier.

use std::sync::Arc;

use domovoi_config::hub::HubConfig;
use domovoi_core::ports::{ChatGateway, Clock, Notifier, OutboundMessage};
use domovoi_core::{Card, CardBadge, HostName, IncidentId, Severity};

use super::AdminError;
use crate::runtime::{AdapterOverrides, adapters, support_public_url};

/// What happened for one gateway or notifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotifyOutcome {
    /// `gateway:<platform>` or `notifier:<id>`.
    pub target: String,
    /// `Err` holds the reason it failed.
    pub result: Result<(), String>,
}

/// Sends a test card to each enabled gateway and a test message to each notifier, built
/// from `config`. Needs no store, so it works next to a running hub. An empty list means
/// nothing is configured.
pub async fn notify_test(
    config: &HubConfig,
    clock: Arc<dyn Clock>,
) -> Result<Vec<NotifyOutcome>, AdminError> {
    let public_url = support_public_url(config)?;
    let (gateways, notifiers) =
        adapters::build_chat(config, &clock, &public_url, &AdapterOverrides::default())?;
    let host = HostName::parse(&config.hub.name)
        .or_else(|_| HostName::parse("domovoi"))
        .map_err(|err| AdminError::Invalid(err.to_string()))?;
    Ok(send_all(&gateways, &notifiers, &host).await)
}

pub(crate) async fn send_all(
    gateways: &[Arc<dyn ChatGateway>],
    notifiers: &[Arc<dyn Notifier>],
    host: &HostName,
) -> Vec<NotifyOutcome> {
    let card = Card {
        incident: IncidentId::new(),
        host: host.clone(),
        badge: CardBadge::Severity(Severity::Info),
        headline: "Test card from domovoi notify test".to_owned(),
        bullets: vec!["If you can read this, cards reach this channel.".to_owned()],
        next_step: None,
        buttons: Vec::new(),
        details: None,
        resolved: true,
    };
    let message = OutboundMessage {
        title: "Domovoi test message".to_owned(),
        markdown: "If you can read this, notifications reach this channel.".to_owned(),
        severity: None,
    };
    let mut outcomes = Vec::new();
    for gateway in gateways {
        outcomes.push(NotifyOutcome {
            target: format!("gateway:{}", gateway.platform()),
            result: gateway
                .post_card(&card)
                .await
                .map(|_| ())
                .map_err(|err| err.to_string()),
        });
    }
    for notifier in notifiers {
        outcomes.push(NotifyOutcome {
            target: format!("notifier:{}", notifier.id()),
            result: notifier.send(&message).await.map_err(|err| err.to_string()),
        });
    }
    outcomes
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use domovoi_core::HostName;
    use domovoi_core::ports::{ChatError, ChatGateway, Notifier};
    use domovoi_testkit::{RecordingGateway, RecordingNotifier};

    use super::send_all;

    #[tokio::test]
    async fn every_gateway_gets_a_card_and_every_notifier_a_message() {
        let gateway = Arc::new(RecordingGateway::new("discord"));
        let up = Arc::new(RecordingNotifier::new("webhook"));
        let down = Arc::new(RecordingNotifier::new("broken"));
        down.fail_next(ChatError::Transport("offline".to_owned()));
        let gateways: Vec<Arc<dyn ChatGateway>> = vec![gateway.clone()];
        let notifiers: Vec<Arc<dyn Notifier>> = vec![up.clone(), down];
        let host = HostName::parse("box").unwrap();
        let outcomes = send_all(&gateways, &notifiers, &host).await;
        assert_eq!(outcomes.len(), 3);
        assert!(outcomes[0].result.is_ok() && outcomes[1].result.is_ok());
        assert!(outcomes[2].result.as_ref().unwrap_err().contains("offline"));
        assert_eq!(up.sent().len(), 1);
    }
}
