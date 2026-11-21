//! A person's own display preferences: how much detail the pages show them.
//!
//! The values are presentation only. They live in the settings table under a key built from
//! the caller's own account id, and nothing here reaches policy, approvals, audit or the
//! engine: no method takes a user id, so a person can only read and write their own row.

use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_core::UserId;
use domovoi_core::ports::{Clock, SettingsRepo, StoreError};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::Caller;

/// The version written into every stored document.
pub const PREFERENCES_VERSION: u32 = 1;

/// The most pages that may have their own detail level.
pub const MAX_PAGE_OVERRIDES: usize = 32;

/// The longest page key, in characters.
pub const MAX_PAGE_KEY_CHARS: usize = 32;

/// How much of a rejected value is echoed back, in characters.
const ECHO_CHARS: usize = 40;

/// The settings key of one person's preferences.
#[must_use]
pub fn preferences_key(user: UserId) -> String {
    format!("preferences.{user}")
}

/// How much detail a page shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    /// Everything the page has.
    #[default]
    Detailed,
    /// The usual amount.
    Standard,
    /// Only what needs attention.
    Essential,
}

impl Profile {
    /// The profile with exactly this lowercase name.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "detailed" => Some(Self::Detailed),
            "standard" => Some(Self::Standard),
            "essential" => Some(Self::Essential),
            _ => None,
        }
    }

    /// The name stored and sent over the API.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Detailed => "detailed",
            Self::Standard => "standard",
            Self::Essential => "essential",
        }
    }
}

/// The detail level everywhere, with per-page exceptions.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PresentationPrefs {
    /// The level pages use unless they have their own.
    pub profile: Profile,
    /// Pages with their own level, by page key.
    pub pages: BTreeMap<String, Profile>,
}

/// One person's stored preferences.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Preferences {
    /// How pages are presented.
    pub presentation: PresentationPrefs,
    /// When the preferences were last saved; `None` while nothing is stored.
    pub updated_at: Option<Timestamp>,
}

/// New presentation settings, as typed by the person and not yet checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentationInput {
    /// The level for pages without their own.
    pub profile: String,
    /// Levels by page key.
    pub pages: BTreeMap<String, String>,
}

/// A change to the preferences; a group left out stays as it is.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PreferencesUpdate {
    /// Replaces the whole presentation group when present.
    pub presentation: Option<PresentationInput>,
}

/// Why preferences could not be read or saved.
#[derive(Debug, thiserror::Error)]
pub enum PreferenceError {
    /// The change was refused; the text says what to fix.
    #[error("{0}")]
    Invalid(String),
    /// The store failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Reads and saves the signed-in person's display preferences.
#[derive(Clone)]
pub struct PreferenceService {
    settings: Arc<dyn SettingsRepo>,
    clock: Arc<dyn Clock>,
    write: Arc<tokio::sync::Mutex<()>>,
}

impl PreferenceService {
    /// A service over `settings`, stamping saves with `clock`.
    #[must_use]
    pub fn new(settings: Arc<dyn SettingsRepo>, clock: Arc<dyn Clock>) -> Self {
        Self {
            settings,
            clock,
            write: Arc::default(),
        }
    }

    /// The caller's preferences; defaults while nothing is stored.
    pub async fn get(&self, caller: &Caller) -> Result<Preferences, PreferenceError> {
        self.read_for(caller.user()).await
    }

    /// Applies `update` to the caller's preferences and returns the result.
    pub async fn update(
        &self,
        caller: &Caller,
        update: PreferencesUpdate,
    ) -> Result<Preferences, PreferenceError> {
        self.write_for(caller.user(), update).await
    }

    async fn read_for(&self, user: UserId) -> Result<Preferences, PreferenceError> {
        let stored = self.settings.get(&preferences_key(user)).await?;
        Ok(stored.as_ref().map(parse_stored).unwrap_or_default())
    }

    async fn write_for(
        &self,
        user: UserId,
        update: PreferencesUpdate,
    ) -> Result<Preferences, PreferenceError> {
        let Some(input) = update.presentation else {
            return self.read_for(user).await;
        };
        let presentation = validate(&input)?;
        let _guard = self.write.lock().await;
        let key = preferences_key(user);
        let mut document = match self.settings.get(&key).await? {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        };
        document.insert("version".to_owned(), json!(PREFERENCES_VERSION));
        document.insert(
            "updated_at".to_owned(),
            Value::String(self.clock.now().to_string()),
        );
        document.insert("presentation".to_owned(), presentation_json(&presentation));
        let value = Value::Object(document);
        self.settings.set(&key, &value).await?;
        Ok(parse_stored(&value))
    }
}

/// `text` cut to what is echoed in a refusal.
fn echo(text: &str) -> String {
    text.chars().take(ECHO_CHARS).collect()
}

fn unknown_profile(text: &str) -> PreferenceError {
    PreferenceError::Invalid(format!(
        "unknown profile `{}`; use detailed, standard or essential",
        echo(text)
    ))
}

/// Whether `key` is 1 to 32 characters of `a-z`, `0-9`, `-` and `_`, starting with a letter.
fn valid_page_key(key: &str) -> bool {
    let mut bytes = key.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    first.is_ascii_lowercase()
        && key.len() <= MAX_PAGE_KEY_CHARS
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// The checked form of `input`, or the first thing wrong with it.
fn validate(input: &PresentationInput) -> Result<PresentationPrefs, PreferenceError> {
    let profile = Profile::parse(&input.profile).ok_or_else(|| unknown_profile(&input.profile))?;
    if input.pages.len() > MAX_PAGE_OVERRIDES {
        return Err(PreferenceError::Invalid(format!(
            "at most {MAX_PAGE_OVERRIDES} pages can have their own detail level"
        )));
    }
    if let Some(bad) = input.pages.keys().find(|key| !valid_page_key(key)) {
        return Err(PreferenceError::Invalid(format!(
            "page key `{}` must start with a lowercase letter and use only lowercase letters, \
             digits, - and _ (up to {MAX_PAGE_KEY_CHARS} characters)",
            echo(bad)
        )));
    }
    let mut pages = BTreeMap::new();
    for (key, text) in &input.pages {
        let level = Profile::parse(text).ok_or_else(|| unknown_profile(text))?;
        pages.insert(key.clone(), level);
    }
    Ok(PresentationPrefs { profile, pages })
}

fn presentation_json(prefs: &PresentationPrefs) -> Value {
    let pages: Map<String, Value> = prefs
        .pages
        .iter()
        .map(|(key, level)| (key.clone(), Value::String(level.name().to_owned())))
        .collect();
    json!({ "profile": prefs.profile.name(), "pages": pages })
}

/// Preferences from whatever is stored, ignoring every damaged part.
fn parse_stored(value: &Value) -> Preferences {
    let Some(map) = value.as_object() else {
        return Preferences::default();
    };
    let updated_at = map
        .get("updated_at")
        .and_then(Value::as_str)
        .and_then(|text| text.parse::<Timestamp>().ok());
    let presentation = map
        .get("presentation")
        .and_then(Value::as_object)
        .map(parse_presentation)
        .unwrap_or_default();
    Preferences {
        presentation,
        updated_at,
    }
}

fn parse_presentation(map: &Map<String, Value>) -> PresentationPrefs {
    let profile = map
        .get("profile")
        .and_then(Value::as_str)
        .and_then(Profile::parse)
        .unwrap_or_default();
    let mut pages = BTreeMap::new();
    if let Some(stored) = map.get("pages").and_then(Value::as_object) {
        let mut entries: Vec<(&String, Profile)> = stored
            .iter()
            .filter(|(key, _)| valid_page_key(key))
            .filter_map(|(key, level)| Some((key, Profile::parse(level.as_str()?)?)))
            .collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        pages.extend(
            entries
                .into_iter()
                .take(MAX_PAGE_OVERRIDES)
                .map(|(key, level)| (key.clone(), level)),
        );
    }
    PresentationPrefs { profile, pages }
}

#[cfg(test)]
mod tests;
