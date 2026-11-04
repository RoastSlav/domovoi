//! Connection tests: one test message, one minimal model call or one ping with candidate
//! values, before they are saved. Nothing is stored. Results are short and never carry
//! a secret.

use std::time::{Duration, Instant};

use domovoi_config::hub::HubConfig;
use domovoi_config::load_hub_layered;
use domovoi_core::HostName;
use domovoi_core::ports::{ChatGateway, LlmMessage, LlmRequest, Part, Role};
use secrecy::ExposeSecret;
use serde_json::{Map, Value};

use super::patch::{self, SecretOp};
use super::service::{SettingsError, SettingsService};
use super::stored::StoredState;

/// How long one test may take.
pub const TEST_TIMEOUT: Duration = Duration::from_secs(15);

/// Which connection to test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Post a test card with the Discord bot.
    Discord,
    /// One minimal call to the primary model.
    Llm,
    /// One GET of the heartbeat URL.
    Heartbeat,
}

/// The result of a test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestResult {
    /// Whether it worked.
    pub ok: bool,
    /// One line: what happened.
    pub message: String,
    /// How long it took.
    pub latency_ms: u64,
}

impl SettingsService {
    /// Tests `target` with the current settings changed by `patch`, which has the same
    /// shape as a save; a secret left as `{ keep: true }` uses the stored value.
    pub async fn test(
        &self,
        target: Target,
        patch: &Map<String, Value>,
    ) -> Result<TestResult, SettingsError> {
        let (config, secrets) = self.candidate(patch).await?;
        let started = Instant::now();
        let outcome = tokio::time::timeout(TEST_TIMEOUT, self.run_test(target, &config)).await;
        let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let (ok, message) = match outcome {
            Ok(Ok(message)) => (true, message),
            Ok(Err(message)) => (false, message),
            Err(_) => (
                false,
                format!("no answer within {} s", TEST_TIMEOUT.as_secs()),
            ),
        };
        Ok(TestResult {
            ok,
            message: scrub(&message, &config, &secrets),
            latency_ms,
        })
    }

    /// Lists models for the candidate primary HTTP provider without saving its settings.
    pub async fn models(&self, patch: &Map<String, Value>) -> Result<Vec<String>, SettingsError> {
        let mut candidate = patch.clone();
        if let Some(primary) = candidate
            .get_mut("llm")
            .and_then(Value::as_object_mut)
            .and_then(|llm| llm.get_mut("primary"))
            .and_then(Value::as_object_mut)
        {
            let model = primary.entry("model").or_insert(Value::Null);
            if model.is_null() {
                *model = Value::String("model-discovery-placeholder".to_owned());
            }
        }
        let (config, secrets) = self.candidate(&candidate).await?;
        let provider =
            config.llm.primary.as_ref().ok_or_else(|| {
                SettingsError::Unavailable("no model provider is set up".to_owned())
            })?;
        let settings = crate::runtime::settings::provider(provider);
        let result = tokio::time::timeout(TEST_TIMEOUT, domovoi_llm::list_models(&settings))
            .await
            .map_err(|_| {
                SettingsError::Unavailable(format!(
                    "model discovery did not answer within {} s",
                    TEST_TIMEOUT.as_secs()
                ))
            })?
            .map_err(|e| {
                SettingsError::Unavailable(scrub(
                    &format!("could not list models: {e}"),
                    &config,
                    &secrets,
                ))
            })?;
        Ok(result)
    }

    async fn candidate(
        &self,
        patch: &Map<String, Value>,
    ) -> Result<(HubConfig, StoredState), SettingsError> {
        let deps = self.deps();
        let mut stored = StoredState::load(&*deps.store, &deps.key).await?;
        let patched = patch::apply(&stored.doc.document, patch).map_err(|e| {
            SettingsError::Invalid(vec![domovoi_config::ConfigIssue::error(e.path, e.message)])
        })?;
        for (path, op) in patched.secrets {
            match op {
                SecretOp::Set(value) => stored.secrets.insert(path, value),
                SecretOp::Clear => stored.secrets.remove(&path),
            };
        }
        stored.doc.document = patched.document;
        let table = stored.doc.table().map_err(SettingsError::Unavailable)?;
        let layered = load_hub_layered(
            deps.sources.file.as_deref(),
            &table,
            &stored,
            &*deps.sources.env,
        )
        .map_err(|err| SettingsError::Invalid(err.issues))?;
        Ok((layered.config, stored))
    }

    async fn run_test(&self, target: Target, config: &HubConfig) -> Result<String, String> {
        let deps = self.deps();
        match target {
            Target::Discord => {
                let discord = config.discord.as_ref().ok_or("Discord is not set up")?;
                let web_base =
                    crate::runtime::support_public_url(config).map_err(|e| e.to_string())?;
                let gateway = domovoi_chat::discord::DiscordGateway::new(
                    "discord-gateway",
                    discord.bot_token.value().clone(),
                    domovoi_chat::discord::DiscordSettings {
                        channel_id: discord.channel_id,
                        web_base,
                    },
                    deps.clock.clone(),
                );
                let host = HostName::parse(&config.hub.name)
                    .or_else(|_| HostName::parse("domovoi"))
                    .map_err(|e| e.to_string())?;
                let card = crate::admin::test_card(&host, "Domovoi test message");
                gateway
                    .post_card(&card)
                    .await
                    .map(|_| format!("posted a test message in channel {}", discord.channel_id))
                    .map_err(|e| format!("Discord refused: {e}"))
            }
            Target::Llm => {
                let provider = config.llm.primary.as_ref().ok_or("no model is set up")?;
                let model = provider
                    .model
                    .clone()
                    .unwrap_or_else(|| "the default model".to_owned());
                let llm = domovoi_llm::build(
                    &crate::runtime::settings::provider(provider),
                    deps.executor.clone(),
                )
                .map_err(|e| e.to_string())?;
                llm.complete(LlmRequest {
                    system: "You are a connection test. Reply with OK.".to_owned(),
                    messages: vec![LlmMessage {
                        role: Role::User,
                        parts: vec![Part::Text("Reply with OK.".to_owned())],
                    }],
                    tools: Vec::new(),
                    max_output_tokens: 16,
                })
                .await
                .map(|_| format!("{model} answered"))
                .map_err(|e| format!("the model call failed: {e}"))
            }
            Target::Heartbeat => {
                let heartbeat = config.heartbeat.as_ref().ok_or("no heartbeat URL is set")?;
                let client = reqwest::Client::builder()
                    .timeout(TEST_TIMEOUT)
                    .user_agent(concat!("domovoi/", env!("CARGO_PKG_VERSION")))
                    .build()
                    .map_err(|e| e.without_url().to_string())?;
                let response = client
                    .get(heartbeat.url.value().expose_secret())
                    .send()
                    .await
                    .map_err(|e| format!("the ping failed: {}", e.without_url()))?;
                let status = response.status();
                if status.is_success() {
                    Ok(format!("the heartbeat URL answered {status}"))
                } else {
                    Err(format!("the heartbeat URL answered {status}"))
                }
            }
        }
    }
}

/// `message` without any secret from the candidate settings.
fn scrub(message: &str, config: &HubConfig, stored: &StoredState) -> String {
    let mut config = config.clone();
    let mut secrets = Vec::new();
    domovoi_config::Secrets::secrets_mut(&mut config, &mut |_, secret| {
        secrets.push(secret.value().clone());
    });
    secrets.extend(stored.secrets.values().cloned());
    let redactor = domovoi_engine::PatternRedactor::new(secrets.iter().cloned());
    let mut out = domovoi_core::ports::Redactor::redact(&redactor, message);
    for secret in &secrets {
        let value = secret.expose_secret();
        if !value.is_empty() {
            out = out.replace(value, "[REDACTED]");
        }
    }
    out
}
