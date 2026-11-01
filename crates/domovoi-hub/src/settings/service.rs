//! The settings service: validates, stores, audits and applies changes made in the web UI.
//! API handlers only translate HTTP to these calls.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use domovoi_config::{ConfigIssue, load_hub_layered};
use domovoi_core::ports::{AuditEvent, AuditLog, Clock, ProcessExecutor, SettingsRepo, StoreError};
use domovoi_core::{CheckKey, Host, User};
use domovoi_proto::messages::{CheckInfo, CheckOverride};
use domovoi_store::Store;
use jiff::{SignedDuration, Timestamp};
use secrecy::ExposeSecret;
use serde_json::{Map, Value, json};
use tokio::sync::mpsc;

use super::apply::{ApplyRequest, ConfigSources, applied, restore_previous};
use super::crypto::{CryptoError, SecretKey};
use super::patch::{self, Change, SecretOp, is_sensitive};
use super::stored::{
    CONFIG, LAST_APPLY, PREVIOUS, SECRET_PREFIX, StoredDoc, StoredState, host_checks_key,
    sealed_value, toml_to_json,
};
use crate::agent_server::AgentServer;
use crate::auth::{AuthError, AuthState};

/// How a secret that has a value is shown.
pub const SET: &str = "[set]";

/// How long a save waits for the supervisor to accept the new settings.
const APPLY_WAIT: Duration = Duration::from_secs(30);
/// Connection tests allowed per session per minute.
pub const TESTS_PER_MINUTE: usize = 10;

/// Why a settings call failed. Messages never contain secret values.
#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    /// The settings changed since the caller read them.
    #[error("settings changed elsewhere (now revision {current}); reload and try again")]
    Conflict {
        /// The stored revision.
        current: u64,
    },
    /// The field is set in `hub.toml` or the environment, which win over the UI.
    #[error("{path} is set by {origin} and cannot be changed here")]
    Locked {
        /// The field.
        path: String,
        /// `file` or `env:<VAR>`.
        origin: String,
    },
    /// The change widens trust and needs the admin's password.
    #[error("enter your password to make this change")]
    PasswordRequired,
    /// The password did not match.
    #[error("wrong password")]
    BadPassword,
    /// Too many wrong passwords from this address; wait this many seconds.
    #[error("too many attempts; try again in {0} s")]
    TooMany(i64),
    /// The change does not make a valid configuration.
    #[error("the settings are not valid")]
    Invalid(Vec<ConfigIssue>),
    /// The settings were valid but the hub could not start with them; the previous ones
    /// are back.
    #[error("the settings could not be applied, so the previous ones are back: {0}")]
    ApplyFailed(String),
    /// A part the request needs is missing.
    #[error("{0}")]
    Unavailable(String),
    /// The store failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// Sealing a secret failed.
    #[error(transparent)]
    Crypto(#[from] CryptoError),
}

/// A saved change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Saved {
    /// The new revision.
    pub revision: u64,
    /// Whether the hub restarted with the change. False when nothing changed, or when the
    /// hub runs without a supervisor (tests); the change then applies at the next start.
    pub applied: bool,
}

/// The settings as the UI shows them.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    /// The stored revision a save must name.
    pub revision: u64,
    /// The merged values, shaped like `hub.toml`, with every secret shown as `[set]`.
    pub values: Value,
    /// The source of every field some layer set: `ui`, `file` or `env:<VAR>`. A field
    /// not listed uses its default; an entry for an array or a secret covers everything
    /// under it.
    pub fields: BTreeMap<String, String>,
    /// Problems with the current layers.
    pub issues: Vec<ConfigIssue>,
    /// How the last apply went, if any.
    pub last_apply: Option<Value>,
}

/// A host's checks: what its agent reported and what the UI overrides.
#[derive(Debug, Clone, PartialEq)]
pub struct HostChecks {
    /// Whether the agent is connected now.
    pub connected: bool,
    /// What the agent reported at its last handshake, while it is connected.
    pub reported: Vec<CheckInfo>,
    /// The overrides saved for the host.
    pub overrides: BTreeMap<CheckKey, CheckOverride>,
}

/// What the service is built from.
pub struct SettingsDeps {
    /// The store holding the UI layer.
    pub store: Arc<Store>,
    /// The data directory's secret key.
    pub key: Arc<SecretKey>,
    /// The file and environment layers.
    pub sources: ConfigSources,
    /// Time.
    pub clock: Arc<dyn Clock>,
    /// Password confirmation.
    pub auth: Arc<AuthState>,
    /// Check overrides are pushed through it.
    pub agents: AgentServer,
    /// The supervisor that applies saved changes; `None` stores them for the next start.
    pub apply: Option<mpsc::Sender<ApplyRequest>>,
    /// Runs a CLI model for the model test.
    pub executor: Option<Arc<dyn ProcessExecutor>>,
}

/// Manages the UI settings layer. Cheap to clone.
#[derive(Clone)]
pub struct SettingsService {
    pub(crate) inner: Arc<Inner>,
}

pub(crate) struct Inner {
    pub(crate) deps: SettingsDeps,
    write: tokio::sync::Mutex<()>,
    tests: Mutex<BTreeMap<[u8; 32], Vec<Timestamp>>>,
}

impl SettingsService {
    /// A service over `deps`.
    #[must_use]
    pub fn new(deps: SettingsDeps) -> Self {
        Self {
            inner: Arc::new(Inner {
                deps,
                write: tokio::sync::Mutex::new(()),
                tests: Mutex::new(BTreeMap::new()),
            }),
        }
    }

    pub(crate) fn deps(&self) -> &SettingsDeps {
        &self.inner.deps
    }

    async fn stored(&self) -> Result<StoredState, SettingsError> {
        Ok(StoredState::load(&*self.deps().store, &self.deps().key).await?)
    }

    /// The settings with their sources, for the settings page.
    pub async fn view(&self) -> Result<View, SettingsError> {
        let deps = self.deps();
        let stored = self.stored().await?;
        let table = stored.doc.table().map_err(SettingsError::Unavailable)?;
        let (values, provenance, issues) = match load_hub_layered(
            deps.sources.file.as_deref(),
            &table,
            &stored,
            &*deps.sources.env,
        ) {
            Ok(layered) => (
                toml_to_json(&toml::Value::Table(layered.merged)),
                layered.provenance,
                layered.issues,
            ),
            Err(err) => (
                Value::Object(stored.doc.document.clone()),
                err.provenance,
                err.issues,
            ),
        };
        Ok(View {
            revision: stored.doc.revision,
            values: mask_secrets(values),
            fields: provenance
                .iter()
                .map(|(path, source)| (path.to_owned(), source.label()))
                .collect(),
            issues,
            last_apply: SettingsRepo::get(&*deps.store, LAST_APPLY).await?,
        })
    }

    /// Saves `patch` on top of revision `base_revision` and applies it (see the module
    /// documentation of `settings` for the steps). `password` is needed when the change
    /// widens trust.
    pub async fn update(
        &self,
        user: &User,
        ip: IpAddr,
        base_revision: u64,
        patch: &Map<String, Value>,
        password: Option<&str>,
    ) -> Result<Saved, SettingsError> {
        let deps = self.deps();
        let _one_at_a_time = self.inner.write.lock().await;
        let stored = self.stored().await?;
        let revision = stored.doc.revision;
        if revision != base_revision {
            return Err(SettingsError::Conflict { current: revision });
        }
        let patched = patch::apply(&stored.doc.document, patch)
            .map_err(|e| SettingsError::Invalid(vec![ConfigIssue::error(e.path, e.message)]))?;
        if patched.changes.is_empty() {
            return Ok(Saved {
                revision,
                applied: false,
            });
        }
        if patched.changes.iter().any(is_sensitive) {
            let password = password.ok_or(SettingsError::PasswordRequired)?;
            deps.auth
                .confirm_password(ip, user.id, password)
                .await
                .map_err(|err| match err {
                    AuthError::LockedOut { retry_after_secs } => {
                        SettingsError::TooMany(retry_after_secs)
                    }
                    _ => SettingsError::BadPassword,
                })?;
        }
        let current_table = stored.doc.table().map_err(SettingsError::Unavailable)?;
        let provenance = match load_hub_layered(
            deps.sources.file.as_deref(),
            &current_table,
            &stored,
            &*deps.sources.env,
        ) {
            Ok(layered) => layered.provenance,
            Err(err) => err.provenance,
        };
        for change in &patched.changes {
            let source = provenance.source_of(change.path());
            if !source.editable() {
                return Err(SettingsError::Locked {
                    path: change.path().to_owned(),
                    origin: source.label(),
                });
            }
        }

        let mut secrets = stored.secrets.clone();
        for (path, op) in &patched.secrets {
            match op {
                SecretOp::Set(value) => secrets.insert(path.clone(), value.clone()),
                SecretOp::Clear => secrets.remove(path),
            };
        }
        let candidate = StoredState {
            doc: StoredDoc {
                revision: revision + 1,
                document: patched.document,
            },
            secrets,
            unreadable: Vec::new(),
        };
        let table = candidate.doc.table().map_err(SettingsError::Unavailable)?;
        let layered = load_hub_layered(
            deps.sources.file.as_deref(),
            &table,
            &candidate,
            &*deps.sources.env,
        )
        .map_err(|err| SettingsError::Invalid(err.issues))?;

        let mut writes = vec![
            (CONFIG.to_owned(), Some(candidate.doc.to_value())),
            (PREVIOUS.to_owned(), Some(stored.doc.to_value())),
        ];
        for (path, op) in &patched.secrets {
            let value = match op {
                SecretOp::Set(secret) => Some(sealed_value(&deps.key, path, secret)?),
                SecretOp::Clear => None,
            };
            writes.push((format!("{SECRET_PREFIX}{path}"), value));
        }
        SettingsRepo::set_many(&*deps.store, &writes).await?;
        self.audit(
            user,
            "settings.changed",
            "settings",
            changes_detail(&patched.changes),
        )
        .await;

        let Some(apply) = &deps.apply else {
            return Ok(Saved {
                revision: revision + 1,
                applied: false,
            });
        };
        let (reply, answer) = tokio::sync::oneshot::channel();
        let sent = apply
            .send(ApplyRequest {
                config: layered.config,
                reply,
            })
            .await;
        let outcome = match sent {
            Ok(()) => match tokio::time::timeout(APPLY_WAIT, answer).await {
                Ok(Ok(outcome)) => outcome,
                _ => Err("the hub did not answer in time".to_owned()),
            },
            Err(_) => Err("the hub is shutting down".to_owned()),
        };
        match outcome {
            Ok(()) => {
                applied(&*deps.store, &*deps.store, &*deps.clock).await;
                Ok(Saved {
                    revision: revision + 1,
                    applied: true,
                })
            }
            Err(why) => {
                let why = redact(&why, &candidate);
                if let Err(err) = restore_previous(&deps.store, &*deps.clock, &why).await {
                    tracing::error!(%err, "could not restore the previous settings");
                }
                Err(SettingsError::ApplyFailed(why))
            }
        }
    }

    /// A host's reported checks and saved overrides.
    pub async fn host_checks(&self, host: &Host) -> Result<HostChecks, SettingsError> {
        let deps = self.deps();
        let reported = deps.agents.reported_checks(host.id);
        Ok(HostChecks {
            connected: reported.is_some(),
            reported: reported.unwrap_or_default(),
            overrides: crate::agent_server::stored_overrides(&*deps.store, host.name.as_str())
                .await,
        })
    }

    /// Saves a host's check overrides and sends them to its agent. Returns whether the
    /// agent got them now; otherwise it gets them at its next handshake. No restart.
    pub async fn set_host_checks(
        &self,
        user: &User,
        host: &Host,
        overrides: BTreeMap<CheckKey, CheckOverride>,
    ) -> Result<bool, SettingsError> {
        let mut issues = Vec::new();
        for (key, item) in &overrides {
            if item.interval_seconds == Some(0) {
                issues.push(ConfigIssue::error(
                    format!("{key}.interval_seconds"),
                    "must be at least 1 second",
                ));
            }
        }
        if !issues.is_empty() {
            return Err(SettingsError::Invalid(issues));
        }
        let deps = self.deps();
        let value = serde_json::to_value(&overrides)
            .map_err(|e| SettingsError::Unavailable(e.to_string()))?;
        SettingsRepo::set(&*deps.store, &host_checks_key(host.name.as_str()), &value).await?;
        self.audit(
            user,
            "settings.host_checks",
            &format!("host:{}", host.name),
            json!({ "overrides": value }),
        )
        .await;
        Ok(deps.agents.push_settings(host.id).await)
    }

    /// Admits one connection test for the session `session`, or says how long to wait.
    pub fn admit_test(&self, session: [u8; 32]) -> Result<(), SettingsError> {
        let now = self.deps().clock.now();
        let since = now.checked_sub(SignedDuration::from_mins(1)).unwrap_or(now);
        let mut tests = self
            .inner
            .tests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let recent = tests.entry(session).or_default();
        recent.retain(|t| *t > since);
        if recent.len() >= TESTS_PER_MINUTE {
            let oldest = recent.first().copied().unwrap_or(now);
            let waited = now.duration_since(oldest).as_secs();
            return Err(SettingsError::TooMany((60 - waited).clamp(1, 60)));
        }
        recent.push(now);
        Ok(())
    }

    async fn audit(&self, user: &User, kind: &str, subject: &str, detail: Value) {
        let deps = self.deps();
        let event = AuditEvent {
            at: deps.clock.now(),
            actor: format!("user:{}", user.id),
            kind: kind.to_owned(),
            subject: subject.to_owned(),
            detail,
        };
        if let Err(err) = AuditLog::append(&*deps.store, &event).await {
            tracing::error!(%err, %kind, "could not write the audit log");
        }
    }
}

/// The audit detail of a change: paths with old and new plain values, and only `set` or
/// `cleared` for secrets.
fn changes_detail(changes: &[Change]) -> Value {
    let items: Vec<Value> = changes
        .iter()
        .map(|change| match change {
            Change::Plain { path, old, new } => json!({ "path": path, "old": old, "new": new }),
            Change::Secret { path, set } => {
                json!({ "path": path, "secret": if *set { "set" } else { "cleared" } })
            }
        })
        .collect();
    json!({ "changes": items })
}

/// Replaces every secret reference (`{ env }`, `{ file }`, `{ stored }`) with `[set]`.
fn mask_secrets(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let is_secret = !map.is_empty()
                && map
                    .keys()
                    .all(|k| matches!(k.as_str(), "env" | "file" | "stored"));
            if is_secret {
                Value::String(SET.to_owned())
            } else {
                Value::Object(map.into_iter().map(|(k, v)| (k, mask_secrets(v))).collect())
            }
        }
        Value::Array(items) => Value::Array(items.into_iter().map(mask_secrets).collect()),
        other => other,
    }
}

/// `text` with every secret of `state` removed.
fn redact(text: &str, state: &StoredState) -> String {
    let redactor = domovoi_engine::PatternRedactor::new(state.secrets.values().cloned());
    let mut out = domovoi_core::ports::Redactor::redact(&redactor, text);
    for secret in state.secrets.values() {
        let value = secret.expose_secret();
        if !value.is_empty() {
            out = out.replace(value, "[REDACTED]");
        }
    }
    out
}
