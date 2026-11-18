//! The raise-only effect-class floors: the hub puts a minimum effect class on one of an
//! agent's skills, saved under `host.<name>.floors` as `{"floors": {"<skill>": "<class>"}}`.
//!
//! Raising is the only write. A skill whose manifest declares `mutate` cannot be made safer
//! by a settings value, so a class lower than the declared one or the current floor is
//! refused with [`FloorWriteError::WouldLower`] and nothing is stored. A malformed stored
//! entry is dropped with one warning and treated as **absent**, never as a lower class, and
//! never fails startup. The floors feed prompt 24's `skill_contract` and `effective_effect`;
//! this module adds no second computation of the effective class. No new table, no
//! migration, no column: one more key in the settings store, like the disabled-skills key.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use domovoi_core::capability::{EffectClass, FloorError, SkillFloor};
use domovoi_core::ports::{SettingsRepo, StoreError};
use domovoi_core::{Host, HostId, SkillName};
use serde_json::json;
use tracing::warn;

/// The settings key of a host's skill floors, like the disabled-skills key.
#[must_use]
pub fn host_floors_key(host: &str) -> String {
    format!("host.{host}.floors")
}

/// Why a floor write is refused.
#[derive(Debug, Clone, PartialEq)]
pub enum FloorWriteError {
    /// The new class is not stricter than the declared class or the current floor.
    WouldLower,
    /// The class asked for is not one a floor may carry (prompt 24's [`FloorError`]).
    Invalid(FloorError),
    /// The settings store refused the write.
    Store(StoreError),
}

/// The class word a floors value may carry, or `None` when it is not one of the three.
///
/// `contain` belongs to typed helper capabilities and an unknown word is never a class, so
/// neither reads as the safe default.
#[must_use]
fn class_word(word: &str) -> Option<EffectClass> {
    match word {
        "read" => Some(EffectClass::Read),
        "mutate" => Some(EffectClass::Mutate),
        "destructive" => Some(EffectClass::Destructive),
        _ => None,
    }
}

/// The raise-only floors, one settings key per host.
pub struct Floors {
    /// Where the per-host JSON is kept.
    pub(crate) store: Arc<dyn SettingsRepo>,
    /// The floor per host and skill.
    pub(crate) raised: Mutex<BTreeMap<HostId, BTreeMap<SkillName, SkillFloor>>>,
    /// Held for the whole of one update, across the store write.
    pub(crate) updates: tokio::sync::Mutex<()>,
}

/// Locks a floors map without panicking on a poisoned lock.
fn lock(
    map: &Mutex<BTreeMap<HostId, BTreeMap<SkillName, SkillFloor>>>,
) -> MutexGuard<'_, BTreeMap<HostId, BTreeMap<SkillName, SkillFloor>>> {
    map.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Floors {
    /// Reads `host.<name>.floors` for each host, dropping malformed entries with a warning.
    /// A key that is not a skill name, a value that is not a class word, `contain` and a
    /// non-object `floors` are each dropped and treated as absent.
    pub async fn load(store: Arc<dyn SettingsRepo>, hosts: &[Host]) -> Result<Floors, StoreError> {
        let mut raised = BTreeMap::<HostId, BTreeMap<SkillName, SkillFloor>>::new();
        for entry in hosts {
            let saved = store.get(&host_floors_key(entry.name.as_str())).await?;
            let field = match saved.as_ref() {
                None => continue,
                Some(value) => value.get("floors"),
            };
            let items = match field {
                None => continue,
                Some(value) => match value.as_object() {
                    None => {
                        warn!(
                            "host {name} has a floors value that is not an object; treating it as absent",
                            name = entry.name.as_str(),
                        );
                        continue;
                    }
                    Some(objects) => objects,
                },
            };
            let mut per_skill = BTreeMap::<SkillName, SkillFloor>::new();
            for (raw_skill, stored) in items.iter() {
                let Some(skill) = SkillName::parse(raw_skill.as_str()).ok() else {
                    warn!(
                        "host {name} has a floors key that is not a skill name",
                        name = entry.name.as_str(),
                    );
                    continue;
                };
                let Some(class) = stored.as_str().and_then(class_word) else {
                    warn!(
                        "host {name} floors {skill} with a value that is not a class word; treating it as absent",
                        name = entry.name.as_str(),
                        skill = skill.as_str(),
                    );
                    continue;
                };
                match SkillFloor::new(class) {
                    Err(error) => {
                        warn!(
                            "host {name} floors {skill} with a class no floor may carry: {error}",
                            name = entry.name.as_str(),
                            skill = skill.as_str(),
                        );
                    }
                    Ok(floor) => {
                        per_skill.insert(skill.clone(), floor);
                    }
                }
            }
            if !per_skill.is_empty() {
                raised.insert(entry.id, per_skill);
            }
        }
        Ok(Self {
            store,
            raised: Mutex::new(raised),
            updates: tokio::sync::Mutex::new(()),
        })
    }

    /// The floor for `skill`, or `None`.
    #[must_use]
    pub fn get(&self, host: HostId, skill: &SkillName) -> Option<SkillFloor> {
        let raised = lock(&self.raised);
        raised
            .get(&host)
            .and_then(|per_skill| per_skill.get(skill).copied())
    }

    /// The stored floors as class words, for the API and the tests.
    #[must_use]
    pub fn names(&self, host: HostId) -> BTreeMap<SkillName, EffectClass> {
        let raised = lock(&self.raised);
        let Some(per_skill) = raised.get(&host) else {
            return BTreeMap::<SkillName, EffectClass>::new();
        };
        per_skill
            .iter()
            .map(|(skill, floor)| (skill.clone(), floor.class()))
            .collect::<BTreeMap<SkillName, EffectClass>>()
    }

    /// Writes a new floor. `Some(floor)` must be stricter than the declared class and than
    /// what the host holds today; `None` clears the floor back to the declared class, which
    /// is not a lowering. Read-modify-write under the update lock, so one update is one
    /// whole step: compute the new map, write the settings value, then publish it. A store
    /// error leaves both the store and the memory as they were.
    pub async fn set(
        &self,
        host: &Host,
        skill: SkillName,
        declared: EffectClass,
        floor: Option<EffectClass>,
    ) -> Result<(), FloorWriteError> {
        let _writing = self.updates.lock().await;
        let wanted = match floor {
            None => None,
            Some(class) => match SkillFloor::new(class) {
                Err(error) => return Err(FloorWriteError::Invalid(error)),
                Ok(raised) => Some(raised),
            },
        };
        if let Some(ground) = wanted {
            let holds = self.get(host.id, &skill).map(|floor| floor.class());
            let below_declared = ground.class() < declared;
            let below_floor = match holds {
                None => false,
                Some(current) => ground.class() < current,
            };
            if below_declared || below_floor {
                return Err(FloorWriteError::WouldLower);
            }
        }
        let names = self.with_floor(host.id, skill, wanted);
        let value = json!({
            "floors": names.iter()
                .map(|(skill, raised)| (
                    skill.as_str().to_owned(),
                    raised.class().as_str().to_owned(),
                ))
                .collect::<BTreeMap<String, String>>(),
        });
        self.store
            .set(&host_floors_key(host.name.as_str()), &value)
            .await
            .map_err(FloorWriteError::Store)?;
        self.publish(host.id, &names);
        Ok(())
    }

    /// The host's floors with `skill` raised to `wanted` or cleared; memory is left alone.
    #[must_use]
    fn with_floor(
        &self,
        host: HostId,
        skill: SkillName,
        wanted: Option<SkillFloor>,
    ) -> BTreeMap<SkillName, SkillFloor> {
        let raised = lock(&self.raised);
        let mut names = match raised.get(&host) {
            None => BTreeMap::<SkillName, SkillFloor>::new(),
            Some(per_skill) => per_skill.clone(),
        };
        if let Some(ground) = wanted {
            names.insert(skill.clone(), ground);
        } else {
            names.remove(&skill);
        }
        names
    }

    /// Publishes `names` as `host`'s floors. A host with none drops out of memory.
    fn publish(&self, host: HostId, names: &BTreeMap<SkillName, SkillFloor>) {
        let mut raised = lock(&self.raised);
        if names.is_empty() {
            raised.remove(&host);
            return;
        }
        raised.insert(host, names.clone());
    }
}

#[cfg(test)]
mod tests {
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
}
