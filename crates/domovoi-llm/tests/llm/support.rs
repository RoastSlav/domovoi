use std::sync::OnceLock;

use domovoi_core::ports::{LlmMessage, LlmRequest, Part, Role};
use domovoi_llm::{ProviderKind, ProviderSettings};
use secrecy::SecretString;
use url::Url;

/// Installs the ring provider once for the whole test binary, as the binary does at startup.
pub fn crypto() {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

pub const KEY: &str = "sk-test-SECRET-123";

pub fn anthropic(base: &str) -> ProviderSettings {
    crypto();
    let mut settings = ProviderSettings::new(ProviderKind::Anthropic);
    settings.base_url = Some(Url::parse(base).unwrap());
    settings.model = Some("claude-test".to_owned());
    settings.api_key = Some(SecretString::from(KEY.to_owned()));
    settings
}

pub fn openai(base: &str, key: bool) -> ProviderSettings {
    crypto();
    let mut settings = ProviderSettings::new(ProviderKind::OpenaiCompatible);
    settings.base_url = Some(Url::parse(&format!("{base}/v1")).unwrap());
    settings.model = Some("llama3".to_owned());
    settings.api_key = key.then(|| SecretString::from(KEY.to_owned()));
    settings
}

pub fn user(text: &str) -> LlmMessage {
    LlmMessage {
        role: Role::User,
        parts: vec![Part::Text(text.to_owned())],
    }
}

pub fn ask(text: &str) -> LlmRequest {
    LlmRequest {
        system: "You triage incidents.".to_owned(),
        messages: vec![user(text)],
        tools: Vec::new(),
        max_output_tokens: 512,
    }
}
