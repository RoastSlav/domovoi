//! Looking for a newer Domovoi release and remembering the last answer.

use std::sync::{Arc, Mutex, RwLock};

use domovoi_core::ports::{Clock, SettingsRepo};
use domovoi_proto::parse_release;
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::every::every;

/// Where GitHub answers with the newest published release.
pub const RELEASE_URL: &str = "https://api.github.com/repos/RoastSlav/domovoi/releases/latest";

/// The settings key the last result is saved under.
const UPDATE_KEY: &str = "update_check";

/// How long the hub waits before its first check, so a restart is not a burst.
const FIRST_CHECK: SignedDuration = SignedDuration::from_mins(1);

/// How often the hub looks for a release while checks are on.
const CHECK_INTERVAL: SignedDuration = SignedDuration::from_hours(12);

/// How long a manual check waits before another is allowed.
const MANUAL_INTERVAL: SignedDuration = SignedDuration::from_secs(60);

/// The newest release the hub knows about, as checked against this hub's version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateInfo {
    /// The version this hub runs.
    pub current: String,
    /// The release's tag, for example `v0.5.0`.
    pub latest: String,
    /// Whether `latest` is newer than `current`.
    pub newer: bool,
    /// The release's page on GitHub.
    pub url: String,
    /// The release notes, cut to 4000 characters.
    pub notes: Option<String>,
    /// When the release was published, as GitHub reports it.
    pub published_at: Option<String>,
    /// When this hub last asked.
    pub checked_at: Timestamp,
}

/// A release check that did not produce a usable answer.
#[derive(Debug, Clone, thiserror::Error)]
pub enum CheckError {
    /// A manual check was made less than a minute ago; retry after this many seconds.
    #[error("checked less than a minute ago; try again in {0} s")]
    TooSoon(i64),
    /// GitHub did not answer with a usable release.
    #[error("{0}")]
    Failed(String),
}

/// Reads and stores the newest release, shared by the scheduler and the API.
#[derive(Clone)]
pub struct UpdateChecker {
    http: Option<reqwest::Client>,
    latest: Arc<RwLock<Option<UpdateInfo>>>,
    store: Arc<dyn SettingsRepo>,
    clock: Arc<dyn Clock>,
    last_manual: Arc<Mutex<Option<Timestamp>>>,
}

impl UpdateChecker {
    /// Builds a checker and loads the result saved by the previous run, if any.
    pub async fn load(store: Arc<dyn SettingsRepo>, clock: Arc<dyn Clock>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .user_agent(concat!("domovoi/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(
                |err| tracing::debug!(err = %err.without_url(), "no HTTP client for update checks"),
            )
            .ok();
        let latest = load_stored(&*store).await;
        Self {
            http,
            latest: Arc::new(RwLock::new(latest)),
            store,
            clock,
            last_manual: Arc::new(Mutex::new(None)),
        }
    }

    /// The last result, when this hub has checked at least once.
    pub fn stored(&self) -> Option<UpdateInfo> {
        self.latest
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Replaces the in-memory result with the one saved in the store.
    pub async fn reload(&self) {
        let stored = load_stored(&*self.store).await;
        *self
            .latest
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = stored;
    }

    /// Checks now, at most once a minute for someone pressing the button.
    pub async fn check_now(&self) -> Result<UpdateInfo, CheckError> {
        let now = self.clock.now();
        {
            let mut last = self
                .last_manual
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(previous) = *last {
                let next = previous.checked_add(MANUAL_INTERVAL).unwrap_or(previous);
                if now < next {
                    return Err(CheckError::TooSoon(
                        next.duration_since(now).as_secs().max(1),
                    ));
                }
            }
            *last = Some(now);
        }
        match self.refresh().await {
            Ok(Some(info)) => Ok(info),
            Ok(None) => Err(CheckError::Failed(
                "GitHub returned no published release".to_owned(),
            )),
            Err(err) => Err(CheckError::Failed(err)),
        }
    }

    /// Asks GitHub and keeps the answer. `Ok(None)` means the answer was not usable, such
    /// as a draft or a prerelease, and the previous result stays.
    async fn refresh(&self) -> Result<Option<UpdateInfo>, String> {
        let http = self
            .http
            .as_ref()
            .ok_or_else(|| "no HTTP client".to_owned())?;
        let release = check(http, RELEASE_URL).await?;
        let Some(info) = evaluate(env!("CARGO_PKG_VERSION"), &release, self.clock.now()) else {
            return Ok(None);
        };
        *self
            .latest
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(info.clone());
        if let Ok(value) = serde_json::to_value(&info)
            && let Err(err) = self.store.set(UPDATE_KEY, &value).await
        {
            tracing::warn!(%err, "could not save the update check");
        }
        Ok(Some(info))
    }
}

/// Asks `url` for a release. The caller supplies the HTTP client so checks share its
/// timeout and user agent.
pub async fn check(http: &reqwest::Client, url: &str) -> Result<Value, String> {
    let response = http
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|err| err.without_url().to_string())?;
    let response = response
        .error_for_status()
        .map_err(|err| err.without_url().to_string())?;
    response
        .json()
        .await
        .map_err(|err| err.without_url().to_string())
}

/// Reads a GitHub release into [`UpdateInfo`]. Drafts, prereleases, releases without a
/// usable tag or URL, and releases that do not parse are ignored.
pub fn evaluate(current: &str, release: &Value, now: Timestamp) -> Option<UpdateInfo> {
    if release.get("draft").and_then(Value::as_bool) == Some(true)
        || release.get("prerelease").and_then(Value::as_bool) == Some(true)
    {
        return None;
    }
    let latest = release.get("tag_name")?.as_str()?;
    let url = release.get("html_url")?.as_str()?;
    let latest_release = parse_release(latest)?;
    let current_release = parse_release(current)?;
    Some(UpdateInfo {
        current: current.to_owned(),
        latest: latest.to_owned(),
        newer: latest_release > current_release,
        url: url.to_owned(),
        notes: release
            .get("body")
            .and_then(Value::as_str)
            .map(|body| body.chars().take(4000).collect()),
        published_at: release
            .get("published_at")
            .and_then(Value::as_str)
            .map(str::to_owned),
        checked_at: now,
    })
}

/// Loads the result saved under [`UPDATE_KEY`]; a missing or unreadable value is `None`.
async fn load_stored(store: &dyn SettingsRepo) -> Option<UpdateInfo> {
    match store.get(UPDATE_KEY).await {
        Ok(Some(value)) => serde_json::from_value(value)
            .map_err(|err| tracing::debug!(%err, "could not read the saved update check"))
            .ok(),
        Ok(None) => None,
        Err(err) => {
            tracing::debug!(%err, "could not load the saved update check");
            None
        }
    }
}

/// Checks once, a minute after start, then every twelve hours. Failures are logged at
/// debug because an offline hub is normal.
pub(crate) fn spawn(
    checker: UpdateChecker,
    clock: Arc<dyn Clock>,
    shutdown: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let first = clock
            .now()
            .checked_add(FIRST_CHECK)
            .unwrap_or_else(|_| clock.now());
        tokio::select! {
            () = shutdown.cancelled() => return,
            () = clock.sleep_until(first) => {}
        }
        every(clock, CHECK_INTERVAL, shutdown, move || {
            let checker = checker.clone();
            async move {
                if let Err(err) = checker.refresh().await {
                    tracing::debug!(%err, "could not check for a new release");
                }
            }
        })
        .await
    })
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;
    use serde_json::json;

    use super::evaluate;

    fn now() -> Timestamp {
        "2026-11-10T12:00:00Z".parse().unwrap()
    }

    fn release(tag: &str) -> serde_json::Value {
        json!({
            "tag_name": tag,
            "html_url": "https://github.com/RoastSlav/domovoi/releases/tag/v0.2.0",
            "body": "Notes",
            "published_at": "2026-11-09T10:00:00Z",
            "draft": false,
            "prerelease": false
        })
    }

    #[test]
    fn a_newer_tag_is_an_update() {
        let info = evaluate("0.1.0", &release("v0.2.0"), now()).unwrap();
        assert!(info.newer);
        assert_eq!(info.latest, "v0.2.0");
        assert_eq!(info.current, "0.1.0");
        assert_eq!(info.checked_at, now());
    }

    #[test]
    fn the_same_tag_is_not_newer() {
        let info = evaluate("0.2.0", &release("v0.2.0"), now()).unwrap();
        assert!(!info.newer);
    }

    #[test]
    fn prereleases_are_ignored() {
        let mut prerelease = release("v0.3.0");
        prerelease["prerelease"] = json!(true);
        assert!(evaluate("0.2.0", &prerelease, now()).is_none());
    }

    #[test]
    fn a_release_without_the_needed_fields_is_ignored() {
        assert!(evaluate("0.2.0", &json!({ "tag_name": "v0.3.0" }), now()).is_none());
        assert!(
            evaluate(
                "0.2.0",
                &json!({ "html_url": "https://example.net" }),
                now()
            )
            .is_none()
        );
    }

    #[test]
    fn release_notes_are_cut_to_4000_characters() {
        let mut long = release("v0.3.0");
        long["body"] = json!("x".repeat(5000));
        let info = evaluate("0.2.0", &long, now()).unwrap();
        assert_eq!(info.notes.unwrap().chars().count(), 4000);
    }
}
