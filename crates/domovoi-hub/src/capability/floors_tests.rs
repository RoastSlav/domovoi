use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_core::capability::{EffectClass, FloorError, SkillFloor};
use domovoi_core::ports::{SettingsRepo, StoreError};
use domovoi_core::{Host, SkillName};
use domovoi_testkit::InMemoryStore;
use domovoi_testkit::build::host;
use serde_json::json;

use super::{FloorWriteError, Floors, class_word, host_floors_key};

const BACKUP: &str = "backup_db";
const PRUNE: &str = "prune_logs";

fn named(name: &str) -> SkillName {
    SkillName::parse(name).unwrap()
}

/// One host, a fresh settings store, and the floors read from it.
async fn fresh() -> (Host, Floors, Arc<dyn SettingsRepo>) {
    let nas = host("nas").unwrap();
    let store: Arc<dyn SettingsRepo> = Arc::new(InMemoryStore::new());
    let floors = Floors::load(store.clone(), std::slice::from_ref(&nas))
        .await
        .unwrap();
    (nas, floors, store)
}

/// The one well-formed entry a malformed settings value must leave behind.
#[must_use]
fn one_entry(skill: &str, class: EffectClass) -> BTreeMap<SkillName, EffectClass> {
    let mut names = BTreeMap::<SkillName, EffectClass>::new();
    names.insert(named(skill), class);
    names
}

#[tokio::test]
async fn malformed_floor_is_treated_as_absent() {
    let (nas, _, store) = fresh().await;
    let stored = json!({
        "floors": {
            "x": "contain",
            "y": "nonsense",
            "z": 3,
            "ok_skill": "mutate",
        },
    });
    store
        .set(&host_floors_key(nas.name.as_str()), &stored)
        .await
        .unwrap();
    let floors = Floors::load(store.clone(), std::slice::from_ref(&nas))
        .await
        .unwrap();
    assert_eq!(
        floors.names(nas.id),
        one_entry("ok_skill", EffectClass::Mutate),
        "the loaded floors are not the one well-formed entry",
    );
    assert_eq!(
        floors.get(nas.id, &named("ok_skill")),
        SkillFloor::new(EffectClass::Mutate).ok(),
        "the well-formed entry did not load",
    );
    assert_eq!(class_word("contain"), None, "contain reads as a class word");
    assert_eq!(
        floors.get(nas.id, &named("x")),
        None,
        "a contained floor loaded as a class",
    );
    assert_eq!(
        floors.get(nas.id, &named("y")),
        None,
        "a nonsense class loaded"
    );
    assert_eq!(
        floors.get(nas.id, &named("z")),
        None,
        "a number loaded as a class"
    );
}

#[tokio::test]
async fn floor_cannot_lower_declared_or_current_class() {
    let (nas, floors, store) = fresh().await;
    let skill = named(BACKUP);
    floors
        .set(
            &nas,
            skill.clone(),
            EffectClass::Read,
            Some(EffectClass::Mutate),
        )
        .await
        .unwrap();
    let below_declared = floors
        .set(
            &nas,
            skill.clone(),
            EffectClass::Mutate,
            Some(EffectClass::Read),
        )
        .await;
    assert_eq!(
        below_declared,
        Err(FloorWriteError::WouldLower),
        "a class below the declared one was accepted",
    );
    let below_current = floors
        .set(
            &nas,
            skill.clone(),
            EffectClass::Read,
            Some(EffectClass::Read),
        )
        .await;
    assert_eq!(
        below_current,
        Err(FloorWriteError::WouldLower),
        "a class below the current floor was accepted",
    );
    let below_destructive = floors
        .set(
            &nas,
            named(PRUNE),
            EffectClass::Destructive,
            Some(EffectClass::Mutate),
        )
        .await;
    assert_eq!(
        below_destructive,
        Err(FloorWriteError::WouldLower),
        "a destructive skill was lowered to mutate",
    );
    assert_eq!(
        floors.get(nas.id, &skill),
        SkillFloor::new(EffectClass::Mutate).ok(),
        "a refusal changed the stored floor",
    );
    assert_eq!(
        store
            .get(&host_floors_key(nas.name.as_str()))
            .await
            .unwrap(),
        Some(json!({ "floors": { (BACKUP): "mutate" } })),
        "a refusal wrote the settings value anyway",
    );
}

#[tokio::test]
async fn floor_rejects_contain() {
    let (nas, floors, store) = fresh().await;
    let refused = floors
        .set(
            &nas,
            named(BACKUP),
            EffectClass::Read,
            Some(EffectClass::Contain),
        )
        .await;
    assert_eq!(
        refused,
        Err(FloorWriteError::Invalid(FloorError::ContainNotAllowed)),
        "contain was accepted as a skill floor",
    );
    assert_eq!(
        store
            .get(&host_floors_key(nas.name.as_str()))
            .await
            .unwrap(),
        None,
        "contain reached the settings store",
    );
    assert_eq!(
        floors.get(nas.id, &named(BACKUP)),
        None,
        "contain reached memory"
    );
}

#[tokio::test]
async fn floor_survives_a_reload() {
    let (nas, floors, store) = fresh().await;
    floors
        .set(
            &nas,
            named(BACKUP),
            EffectClass::Read,
            Some(EffectClass::Mutate),
        )
        .await
        .unwrap();
    let reloaded = Floors::load(store.clone(), std::slice::from_ref(&nas))
        .await
        .unwrap();
    assert_eq!(
        reloaded.get(nas.id, &named(BACKUP)),
        SkillFloor::new(EffectClass::Mutate).ok(),
        "a saved floor did not survive the reload",
    );
    assert_eq!(
        reloaded.names(nas.id),
        floors.names(nas.id),
        "the reloaded floors differ from the in-memory ones",
    );
}

#[tokio::test]
async fn two_floor_writes_on_one_host_keep_both() {
    let nas = host("nas").unwrap();
    let store: Arc<dyn SettingsRepo> = Arc::new(InMemoryStore::new());
    let floors = Arc::new(
        Floors::load(store.clone(), std::slice::from_ref(&nas))
            .await
            .unwrap(),
    );
    let (first, second) = tokio::join!(
        tokio::spawn({
            let switch = floors.clone();
            let one = nas.clone();
            async move {
                switch
                    .set(
                        &one,
                        named(BACKUP),
                        EffectClass::Read,
                        Some(EffectClass::Mutate),
                    )
                    .await
            }
        }),
        tokio::spawn({
            let switch = floors.clone();
            let one = nas.clone();
            async move {
                switch
                    .set(
                        &one,
                        named(PRUNE),
                        EffectClass::Read,
                        Some(EffectClass::Destructive),
                    )
                    .await
            }
        }),
    );
    first.unwrap().unwrap();
    second.unwrap().unwrap();
    let saved = store
        .get(&host_floors_key(nas.name.as_str()))
        .await
        .unwrap();
    assert_eq!(
        saved,
        Some(json!({ "floors": { (BACKUP): "mutate", (PRUNE): "destructive" } })),
        "one of the writes overwrote the other",
    );
}

#[tokio::test]
async fn a_store_that_refuses_the_write_leaves_floors_unchanged() {
    let nas = host("nas").unwrap();
    let fresh = Arc::new(InMemoryStore::new());
    let store: Arc<dyn SettingsRepo> = fresh.clone();
    let floors = Floors::load(store.clone(), std::slice::from_ref(&nas))
        .await
        .unwrap();
    fresh.fail_next(StoreError::Backend("read-only disk".to_owned()));
    let refused = floors
        .set(
            &nas,
            named(BACKUP),
            EffectClass::Read,
            Some(EffectClass::Mutate),
        )
        .await;
    assert_eq!(
        refused,
        Err(FloorWriteError::Store(StoreError::Backend(
            "read-only disk".to_owned()
        ))),
        "the store refused the write",
    );
    assert_eq!(
        floors.get(nas.id, &named(BACKUP)),
        None,
        "memory kept the refused floor"
    );
    assert_eq!(
        store
            .get(&host_floors_key(nas.name.as_str()))
            .await
            .unwrap(),
        None,
        "nothing was saved",
    );
}
