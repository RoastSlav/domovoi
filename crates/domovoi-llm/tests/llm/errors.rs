use std::io;
use std::sync::{Arc, Mutex};

use domovoi_core::ports::{LlmError, LlmProvider};
use domovoi_llm::anthropic::Anthropic;
use domovoi_llm::{LlmBuildError, ProviderKind, ProviderSettings, build};
use jiff::SignedDuration;
use secrecy::SecretString;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::support::{KEY, anthropic, ask, crypto, openai};

async fn answer(template: ResponseTemplate) -> Result<(), LlmError> {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(template)
        .mount(&server)
        .await;
    let provider = build(&anthropic(&server.uri()), None).unwrap();
    provider.complete(ask("hello")).await.map(|_| ())
}

#[tokio::test]
async fn rate_limit_maps_retry_after_seconds_and_http_date() {
    assert_eq!(
        answer(ResponseTemplate::new(429).insert_header("retry-after", "7")).await,
        Err(LlmError::RateLimited {
            retry_after: Some(SignedDuration::from_secs(7))
        })
    );
    assert_eq!(
        answer(
            ResponseTemplate::new(429)
                .insert_header("date", "Sat, 31 Oct 2026 12:00:00 GMT")
                .insert_header("retry-after", "Sat, 31 Oct 2026 12:00:30 GMT")
        )
        .await,
        Err(LlmError::RateLimited {
            retry_after: Some(SignedDuration::from_secs(30))
        }),
        "an HTTP date is measured against the response's date"
    );
    assert_eq!(
        answer(
            ResponseTemplate::new(429)
                .insert_header("date", "Sat, 31 Oct 2026 12:00:30 GMT")
                .insert_header("retry-after", "Sat, 31 Oct 2026 12:00:00 GMT")
        )
        .await,
        Err(LlmError::RateLimited {
            retry_after: Some(SignedDuration::ZERO)
        }),
        "a date already passed means retry now"
    );
    assert_eq!(
        answer(ResponseTemplate::new(429)).await,
        Err(LlmError::RateLimited { retry_after: None })
    );
}

#[tokio::test]
async fn auth_and_server_errors_map_to_variants() {
    for status in [401, 403] {
        assert_eq!(
            answer(ResponseTemplate::new(status)).await,
            Err(LlmError::Auth)
        );
    }
    for status in [408, 500, 503, 529] {
        assert!(
            matches!(
                answer(ResponseTemplate::new(status).set_body_string("overloaded")).await,
                Err(LlmError::Unavailable(m)) if m.contains(&status.to_string()) && m.contains("overloaded")
            ),
            "{status}"
        );
    }
    assert!(matches!(
        answer(ResponseTemplate::new(200).set_body_string("<html>proxy</html>")).await,
        Err(LlmError::InvalidResponse(m)) if m.contains("<html>proxy</html>")
    ));
    assert!(matches!(
        answer(ResponseTemplate::new(400).set_body_string("bad model")).await,
        Err(LlmError::InvalidResponse(m)) if m.contains("bad model")
    ));

    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let uri = format!("http://{}", closed.local_addr().unwrap());
    drop(closed);
    let provider = build(&openai(&uri, false), None).unwrap();
    assert!(matches!(
        provider.complete(ask("hello")).await,
        Err(LlmError::Unavailable(_))
    ));
}

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl io::Write for Captured {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn api_key_never_appears_in_debug_or_errors() {
    // Global, not scoped: a scoped subscriber can miss events whose callsites other test
    // threads registered first. Only this test installs one; it sees every test's events.
    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    tracing::subscriber::set_global_default(subscriber).unwrap();

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(400).set_body_string(format!("invalid x-api-key: {KEY}")),
        )
        .mount(&server)
        .await;
    let settings = anthropic(&server.uri());
    let provider = Anthropic::new(
        settings.base_url.as_ref(),
        "claude-test".to_owned(),
        SecretString::from(KEY.to_owned()),
        None,
        settings.timeout,
    )
    .unwrap();

    let echoed = provider.complete(ask("hello")).await.unwrap_err();
    let mut texts = vec![
        format!("{settings:?}"),
        format!("{provider:?}"),
        format!("{echoed:?}"),
        echoed.to_string(),
    ];
    assert!(texts[2].contains("[redacted]"), "{}", texts[2]);

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(401).set_body_string(KEY))
        .mount(&server)
        .await;
    let refused = build(&anthropic(&server.uri()), None)
        .unwrap()
        .complete(ask("hello"))
        .await
        .unwrap_err();
    assert_eq!(refused, LlmError::Auth);
    texts.push(format!("{refused:?}"));

    let logs = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    assert!(
        logs.contains("llm response"),
        "tracing was captured: {logs}"
    );
    texts.push(logs);
    for text in &texts {
        assert!(!text.contains(KEY), "the key leaked into: {text}");
    }
}

#[test]
fn factory_rejects_missing_fields_and_executor() {
    crypto();
    let err = |settings: &ProviderSettings| build(settings, None).err();
    let mut no_key = anthropic("http://127.0.0.1:9");
    no_key.api_key = None;
    assert_eq!(err(&no_key), Some(LlmBuildError::MissingApiKey));
    let mut no_model = anthropic("http://127.0.0.1:9");
    no_model.model = None;
    assert_eq!(err(&no_model), Some(LlmBuildError::MissingModel));
    let mut no_url = openai("http://127.0.0.1:9", false);
    no_url.base_url = None;
    assert_eq!(err(&no_url), Some(LlmBuildError::MissingBaseUrl));

    let mut cli = ProviderSettings::new(ProviderKind::Cli);
    cli.cli = Some(domovoi_llm::CliPreset::claude("/usr/bin/claude"));
    assert_eq!(err(&cli), Some(LlmBuildError::MissingExecutor));
}
