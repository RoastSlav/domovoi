use domovoi_core::UserId;
use domovoi_core::ports::{LlmError, Redactor};
use domovoi_engine::PatternRedactor;
use domovoi_engine::redact::REDACTED;
use secrecy::SecretString;

use crate::harness::{SECRET, Setup, world};

#[test]
fn pattern_redactor_removes_secrets_and_credential_shapes() {
    let redactor = PatternRedactor::new([
        SecretString::from("short".to_owned()),
        SecretString::from("short-and-longer".to_owned()),
        SecretString::from(String::new()),
    ]);
    let samples = [
        "Authorization: Bearer eyJabc.def/ghi+jk==",
        "key sk-abcdefghijklmnopqrstuvwxyz012345",
        "ghp_abcdefghijklmnopqrstuvwxyz0123456789",
        &format!("github_pat_{}", "a".repeat(60)),
        "aws AKIAABCDEFGHIJKLMNOP",
        "slack xoxb-1234567890-abcdef",
        "hook https://discord.com/api/webhooks/123/abc-def",
        "jwt eyJhbGciOi.eyJzdWIiOi.c2lnbmF0dXJl",
        "-----BEGIN RSA PRIVATE KEY-----\nMIIEow\n-----END RSA PRIVATE KEY-----",
    ];
    for sample in samples {
        let out = redactor.redact(sample);
        assert!(out.contains("[REDACTED]"), "{sample} -> {out}");
        assert_eq!(redactor.redact(&out), out, "idempotent for {sample}");
    }
    assert_eq!(
        redactor.redact("value short-and-longer and short"),
        "value [REDACTED] and [REDACTED]",
        "longest secret first"
    );
    assert_eq!(
        redactor.redact("password = hunter2, API_KEY: abc123 token=t0k"),
        "password = [REDACTED] API_KEY: [REDACTED] token=[REDACTED]"
    );
    assert_eq!(
        redactor.redact("nothing to hide here"),
        "nothing to hide here"
    );
}

/// A provider failure the engine reports about: a bearer token and a configured secret,
/// both of which the provider is free to echo back.
fn provider_failure() -> LlmError {
    LlmError::Transport(format!(
        "upstream answered with Bearer abc.def and header {SECRET}"
    ))
}

#[tokio::test]
async fn a_failed_follow_up_never_shows_a_secret_from_the_provider_error() {
    let w = world(Setup::default()).await;
    w.primary.push_err(provider_failure());

    let failed = w
        .triage
        .follow_up(w.incident.id(), "Why does nginx restart?", UserId::new())
        .await
        .unwrap_err()
        .to_string();
    assert!(
        failed.contains(REDACTED),
        "the reason carries the marker in place of what was redacted: {failed}",
    );
    assert!(
        !failed.contains(SECRET) && !failed.contains("abc.def"),
        "neither the configured secret nor the provider's token is shown: {failed}",
    );
}

#[tokio::test]
async fn a_failed_chat_never_shows_a_secret_from_the_provider_error() {
    let w = world(Setup::default()).await;
    w.primary.push_err(provider_failure());

    let failed = w
        .triage
        .chat(None, "Two hosts.", &Vec::new(), "Anything wrong?")
        .await
        .unwrap_err()
        .to_string();
    assert!(
        failed.contains(REDACTED),
        "the reason carries the marker in place of what was redacted: {failed}",
    );
    assert!(
        !failed.contains(SECRET) && !failed.contains("abc.def"),
        "neither the configured secret nor the provider's token is shown: {failed}",
    );
}
