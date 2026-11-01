//! Posts a real card to a test channel. Ignored by default; run with
//! `DOMOVOI_DISCORD_TEST_TOKEN` and `DOMOVOI_DISCORD_TEST_CHANNEL` set:
//!
//! ```sh
//! cargo test -p domovoi-chat --test discord live -- --ignored
//! ```

use std::sync::Arc;

use domovoi_chat::discord::{DiscordGateway, DiscordSettings};
use domovoi_core::ports::ChatGateway;
use domovoi_core::{Card, CardBadge, CardButton, HostName, IncidentId, Verdict};
use domovoi_testkit::FakeClock;
use secrecy::SecretString;
use url::Url;

#[tokio::test]
#[ignore = "posts to a real Discord channel"]
async fn live_card_thread_and_reply() {
    let (Ok(token), Ok(channel)) = (
        std::env::var("DOMOVOI_DISCORD_TEST_TOKEN"),
        std::env::var("DOMOVOI_DISCORD_TEST_CHANNEL"),
    ) else {
        return;
    };
    let _ = rustls::crypto::ring::default_provider().install_default();
    let gateway = DiscordGateway::new(
        SecretString::from(token),
        DiscordSettings {
            channel_id: channel.parse().unwrap(),
            web_base: Url::parse("https://domovoi.example.net").unwrap(),
        },
        Arc::new(FakeClock::new(jiff::Timestamp::now())),
    );
    let mut card = Card {
        incident: IncidentId::new(),
        host: HostName::parse("smoke-test").unwrap(),
        badge: CardBadge::Verdict(Verdict::Monitor),
        headline: "Domovoi smoke test".to_owned(),
        bullets: vec!["Posted by the live test".to_owned()],
        next_step: Some("Nothing; delete this thread".to_owned()),
        buttons: vec![CardButton::Ack, CardButton::Mute1h, CardButton::Details],
        details: None,
        resolved: false,
    };
    let message = gateway.post_card(&card).await.unwrap();
    let thread = gateway
        .open_thread(&message, "Domovoi smoke test")
        .await
        .unwrap();
    assert_eq!(
        gateway.open_thread(&message, "again").await.unwrap(),
        thread,
        "a second open returns the same thread"
    );
    gateway
        .reply(&thread, &"A long reply line.\n".repeat(150))
        .await
        .unwrap();
    card.resolved = true;
    gateway.update_card(&message, &card).await.unwrap();
}
