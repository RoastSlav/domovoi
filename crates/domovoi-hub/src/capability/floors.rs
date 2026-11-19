//! The raise-only effect-class floors: the hub puts a minimum effect class on one of an
//! agent's skills, saved under `host.<name>.floors` as `{"floors": {"<skill>": "<class>"}}`.
//!
//! Raising is the only write. A skill whose manifest declares `mutate` cannot be made safer
//! by a settings value, so a class lower than the declared one or the current floor is
//! refused with [`FloorWriteError::WouldLower`] and nothing is stored. A malformed stored
//! entry is dropped with one warning and treated as **absent**, never as a lower class, and
//! never fails startup. The floors feed `skill_contract` and `effective_effect`;
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
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum FloorWriteError {
    /// The new class is not stricter than the declared class or the current floor.
    #[error("the requested class is not stricter than the declared or current class")]
    WouldLower,
    /// The class asked for is not one a floor may carry (the capability module's [`FloorError`]).
    #[error(transparent)]
    Invalid(#[from] FloorError),
    /// The settings store refused the write.
    #[error(transparent)]
    Store(#[from] StoreError),
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
#[path = "floors_tests.rs"]
mod tests;
