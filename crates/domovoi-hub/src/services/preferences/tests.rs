use async_trait::async_trait;
use domovoi_core::ports::SettingsRepo;
use domovoi_testkit::{FakeClock, InMemoryStore};
use serde_json::{Value, json};

use super::*;

fn t0() -> Timestamp {
    "2026-03-01T10:00:00Z".parse().unwrap()
}

struct Fixture {
    store: Arc<InMemoryStore>,
    clock: Arc<FakeClock>,
    service: PreferenceService,
}

fn fixture() -> Fixture {
    let store = Arc::new(InMemoryStore::new());
    let clock = Arc::new(FakeClock::new(t0()));
    let service = PreferenceService::new(store.clone(), clock.clone());
    Fixture {
        store,
        clock,
        service,
    }
}

fn input(profile: &str, pages: &[(&str, &str)]) -> PreferencesUpdate {
    PreferencesUpdate {
        presentation: Some(PresentationInput {
            profile: profile.to_owned(),
            pages: pages
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        }),
    }
}

fn message(err: PreferenceError) -> String {
    match err {
        PreferenceError::Invalid(text) => text,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

async fn row(f: &Fixture, user: UserId) -> Option<Value> {
    f.store.get(&preferences_key(user)).await.unwrap()
}

async fn rows(f: &Fixture) -> Vec<(String, Value)> {
    f.store.list_prefix("").await.unwrap()
}

#[tokio::test]
async fn a_person_without_a_stored_row_gets_the_defaults() {
    let f = fixture();
    let got = f.service.read_for(UserId::new()).await.unwrap();
    assert_eq!(got, Preferences::default());
    assert_eq!(got.presentation.profile, Profile::Detailed);
    assert!(got.presentation.pages.is_empty());
    assert_eq!(got.updated_at, None);
    assert!(rows(&f).await.is_empty(), "reading writes nothing");
}

#[tokio::test]
async fn update_stores_the_profile_and_page_overrides_and_get_returns_them() {
    let f = fixture();
    let user = UserId::new();
    let saved = f
        .service
        .write_for(user, input("standard", &[("operation", "essential")]))
        .await
        .unwrap();
    assert_eq!(saved.presentation.profile, Profile::Standard);
    assert_eq!(
        saved.presentation.pages.get("operation"),
        Some(&Profile::Essential)
    );
    assert_eq!(f.service.read_for(user).await.unwrap(), saved);
    assert_eq!(
        row(&f, user).await.unwrap(),
        json!({
            "version": 1,
            "updated_at": t0().to_string(),
            "presentation": {"profile": "standard", "pages": {"operation": "essential"}},
        })
    );
}

#[tokio::test]
async fn update_stamps_the_time_from_the_clock() {
    let f = fixture();
    let user = UserId::new();
    let first = f.service.write_for(user, input("standard", &[])).await;
    assert_eq!(first.unwrap().updated_at, Some(t0()));
    f.clock.advance(jiff::SignedDuration::from_mins(5));
    let second = f.service.write_for(user, input("essential", &[])).await;
    assert_eq!(
        second.unwrap().updated_at,
        Some(t0() + jiff::SignedDuration::from_mins(5))
    );
}

#[tokio::test]
async fn update_replaces_the_presentation_group_and_keeps_other_stored_keys() {
    let f = fixture();
    let user = UserId::new();
    f.store
        .set(
            &preferences_key(user),
            &json!({
                "version": 0,
                "future_group": {"x": 1},
                "presentation": {"profile": "essential", "pages": {"old": "standard"}},
            }),
        )
        .await
        .unwrap();
    f.service
        .write_for(user, input("standard", &[("new", "detailed")]))
        .await
        .unwrap();
    let stored = row(&f, user).await.unwrap();
    assert_eq!(stored["future_group"], json!({"x": 1}));
    assert_eq!(stored["version"], 1);
    assert_eq!(
        stored["presentation"],
        json!({"profile": "standard", "pages": {"new": "detailed"}})
    );
}

#[tokio::test]
async fn update_with_no_presentation_changes_nothing() {
    let f = fixture();
    let user = UserId::new();
    let untouched = f
        .service
        .write_for(user, PreferencesUpdate::default())
        .await
        .unwrap();
    assert_eq!(untouched, Preferences::default());
    assert!(rows(&f).await.is_empty());

    let saved = f
        .service
        .write_for(user, input("essential", &[]))
        .await
        .unwrap();
    let before = row(&f, user).await;
    f.clock.advance(jiff::SignedDuration::from_hours(1));
    let again = f
        .service
        .write_for(user, PreferencesUpdate::default())
        .await
        .unwrap();
    assert_eq!(again, saved);
    assert_eq!(
        serde_json::to_vec(&row(&f, user).await).unwrap(),
        serde_json::to_vec(&before).unwrap()
    );
}

#[tokio::test]
async fn update_rejects_an_unknown_profile_and_writes_nothing() {
    let f = fixture();
    for bad in ["compact", "Detailed", ""] {
        let text = message(
            f.service
                .write_for(UserId::new(), input(bad, &[]))
                .await
                .unwrap_err(),
        );
        assert_eq!(
            text,
            format!("unknown profile `{bad}`; use detailed, standard or essential")
        );
    }
    assert!(rows(&f).await.is_empty());
}

#[tokio::test]
async fn update_rejects_a_malformed_page_key() {
    let f = fixture();
    let long = "a".repeat(33);
    for bad in ["Operation", "1op", "", "a b", long.as_str(), "é"] {
        let text = message(
            f.service
                .write_for(UserId::new(), input("standard", &[(bad, "essential")]))
                .await
                .unwrap_err(),
        );
        assert!(text.starts_with("page key `"), "{bad:?}: {text}");
        assert!(
            text.contains("must start with a lowercase letter"),
            "{text}"
        );
    }
    assert!(rows(&f).await.is_empty());
}

#[tokio::test]
async fn update_accepts_a_well_formed_page_key() {
    let f = fixture();
    let max = "a".repeat(32);
    for good in ["op-1_x", "a", max.as_str()] {
        let saved = f
            .service
            .write_for(UserId::new(), input("standard", &[(good, "essential")]))
            .await
            .unwrap();
        assert_eq!(
            saved.presentation.pages.get(good),
            Some(&Profile::Essential)
        );
    }
}

#[tokio::test]
async fn update_rejects_more_than_32_pages() {
    let f = fixture();
    let update = |n: usize| {
        let owned: Vec<String> = (0..n).map(|i| format!("p{i}")).collect();
        let refs: Vec<(&str, &str)> = owned.iter().map(|k| (k.as_str(), "standard")).collect();
        input("standard", &refs)
    };
    let text = message(
        f.service
            .write_for(UserId::new(), update(33))
            .await
            .unwrap_err(),
    );
    assert_eq!(text, "at most 32 pages can have their own detail level");
    let saved = f
        .service
        .write_for(UserId::new(), update(32))
        .await
        .unwrap();
    assert_eq!(saved.presentation.pages.len(), 32);
}

#[tokio::test]
async fn update_rejects_an_unknown_profile_for_a_page() {
    let f = fixture();
    let text = message(
        f.service
            .write_for(UserId::new(), input("standard", &[("operation", "tiny")]))
            .await
            .unwrap_err(),
    );
    assert_eq!(
        text,
        "unknown profile `tiny`; use detailed, standard or essential"
    );
    assert!(rows(&f).await.is_empty());
}

#[tokio::test]
async fn echoed_bad_input_is_cut_to_40_characters() {
    let f = fixture();
    let long = "x".repeat(100);
    let text = message(
        f.service
            .write_for(UserId::new(), input(&long, &[]))
            .await
            .unwrap_err(),
    );
    assert!(text.contains(&format!("`{}`", "x".repeat(40))), "{text}");
    let text = message(
        f.service
            .write_for(UserId::new(), input("standard", &[("y", long.as_str())]))
            .await
            .unwrap_err(),
    );
    assert!(text.contains(&format!("`{}`", "x".repeat(40))), "{text}");
    let text = message(
        f.service
            .write_for(
                UserId::new(),
                input("standard", &[(long.as_str(), "standard")]),
            )
            .await
            .unwrap_err(),
    );
    assert!(text.contains(&"x".repeat(40)), "{text}");
    assert!(!text.contains(&"x".repeat(41)), "{text}");
}

#[tokio::test]
async fn reading_a_damaged_row_gives_defaults_for_the_damaged_parts() {
    let f = fixture();
    let user = UserId::new();
    for damaged in [
        json!("text"),
        json!([]),
        json!({}),
        json!({"presentation": 3}),
        json!({"presentation": {"profile": 7}}),
        json!({"presentation": {"profile": "x"}}),
    ] {
        f.store.set(&preferences_key(user), &damaged).await.unwrap();
        let got = f.service.read_for(user).await.unwrap();
        assert_eq!(got.presentation, PresentationPrefs::default(), "{damaged}");
        assert_eq!(
            row(&f, user).await.unwrap(),
            damaged,
            "reading never writes"
        );
    }
    f.store
        .set(
            &preferences_key(user),
            &json!({"updated_at": "yesterday", "presentation": {"profile": "essential"}}),
        )
        .await
        .unwrap();
    let got = f.service.read_for(user).await.unwrap();
    assert_eq!(got.updated_at, None);
    assert_eq!(got.presentation.profile, Profile::Essential);
}

#[tokio::test]
async fn reading_drops_overrides_with_unknown_profiles_or_bad_keys() {
    let f = fixture();
    let user = UserId::new();
    f.store
        .set(
            &preferences_key(user),
            &json!({"presentation": {"profile": "standard", "pages": {
                "good": "essential", "Bad": "essential", "x": "huge", "y": 4,
            }}}),
        )
        .await
        .unwrap();
    let got = f.service.read_for(user).await.unwrap();
    assert_eq!(
        got.presentation.pages.into_iter().collect::<Vec<_>>(),
        vec![("good".to_owned(), Profile::Essential)]
    );
}

#[tokio::test]
async fn reading_keeps_at_most_32_overrides_in_key_order() {
    let f = fixture();
    let user = UserId::new();
    let pages: serde_json::Map<String, Value> = (0..40)
        .map(|i| (format!("p{i:02}"), json!("essential")))
        .collect();
    f.store
        .set(
            &preferences_key(user),
            &json!({"presentation": {"profile": "standard", "pages": pages}}),
        )
        .await
        .unwrap();
    let got = f.service.read_for(user).await.unwrap();
    let keys: Vec<_> = got.presentation.pages.keys().cloned().collect();
    let expected: Vec<_> = (0..32).map(|i| format!("p{i:02}")).collect();
    assert_eq!(keys, expected);
}

#[tokio::test]
async fn one_person_cannot_see_or_change_another_persons_preferences() {
    let f = fixture();
    let (ann, bob) = (UserId::new(), UserId::new());
    assert_ne!(preferences_key(ann), preferences_key(bob));
    f.service
        .write_for(ann, input("essential", &[("operation", "standard")]))
        .await
        .unwrap();
    assert_eq!(
        f.service.read_for(bob).await.unwrap(),
        Preferences::default()
    );
    f.service
        .write_for(bob, input("standard", &[]))
        .await
        .unwrap();
    let ann_now = f.service.read_for(ann).await.unwrap();
    assert_eq!(ann_now.presentation.profile, Profile::Essential);
    assert_eq!(ann_now.presentation.pages.len(), 1);
    assert_eq!(rows(&f).await.len(), 2);
}

#[tokio::test]
async fn concurrent_updates_keep_a_whole_valid_document() {
    let f = fixture();
    let user = UserId::new();
    let (a, b) = (f.service.clone(), f.service.clone());
    let one = async move {
        a.write_for(user, input("essential", &[("one", "standard")]))
            .await
    };
    let two = async move {
        b.write_for(user, input("standard", &[("two", "essential")]))
            .await
    };
    let (one, two) = tokio::join!(one, two);
    one.unwrap();
    two.unwrap();
    let got = f.service.read_for(user).await.unwrap();
    let keys: Vec<&str> = got.presentation.pages.keys().map(String::as_str).collect();
    let only_one = got.presentation.profile == Profile::Essential && keys == ["one"];
    let only_two = got.presentation.profile == Profile::Standard && keys == ["two"];
    assert!(only_one || only_two, "{got:?}");
    assert_eq!(rows(&f).await.len(), 1);
}

#[tokio::test]
async fn old_database_without_preferences_reads_as_defaults() {
    let f = fixture();
    f.store.set("hub.name", &json!("home")).await.unwrap();
    f.store
        .set("retention.signals_days", &json!(30))
        .await
        .unwrap();
    let got = f.service.read_for(UserId::new()).await.unwrap();
    assert_eq!(got, Preferences::default());
    assert_eq!(rows(&f).await.len(), 2);
}

struct BrokenStore;

#[async_trait]
impl SettingsRepo for BrokenStore {
    async fn get(&self, _key: &str) -> Result<Option<Value>, StoreError> {
        Err(StoreError::Backend("disk gone".to_owned()))
    }
    async fn set(&self, _key: &str, _value: &Value) -> Result<(), StoreError> {
        Ok(())
    }
    async fn set_many(&self, _writes: &[(String, Option<Value>)]) -> Result<(), StoreError> {
        Ok(())
    }
    async fn list_prefix(&self, _prefix: &str) -> Result<Vec<(String, Value)>, StoreError> {
        Ok(Vec::new())
    }
}

#[tokio::test]
async fn the_store_failing_is_an_error_not_defaults() {
    let clock = Arc::new(FakeClock::new(t0()));
    let service = PreferenceService::new(Arc::new(BrokenStore), clock);
    let user = UserId::new();
    assert!(matches!(
        service.read_for(user).await,
        Err(PreferenceError::Store(_))
    ));
    assert!(matches!(
        service.write_for(user, input("standard", &[])).await,
        Err(PreferenceError::Store(_))
    ));
}
