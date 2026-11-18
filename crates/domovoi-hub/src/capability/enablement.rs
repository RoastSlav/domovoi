//! Which skills the hub may use on each host: the UI's per-host switch, saved in the
//! settings table under `host.<name>.skills` as `{"disabled": ["name", ..]}`.
//!
//! The owner's decision, stored per host, and the only reader and writer of that key: the
//! registry reads it and the API reads it through the registry, so nothing else parses the
//! key. Enforcement at the final boundary is unchanged in kind - `collect_evidence` and
//! `execute` ask the registry whether a skill is enabled and refuse with [`DISABLED_REASON`]
//! in exactly the words the hub used before.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use domovoi_core::ports::{SettingsRepo, StoreError};
use domovoi_core::skill::SkillName;
use domovoi_core::{Host, HostId};
use serde_json::Value;
use serde_json::json;

/// Why the hub refuses a disabled skill.
pub const DISABLED_REASON: &str = "the skill is disabled on this host";

/// The settings key of a host's disabled skills, like the check overrides key.
#[must_use]
pub fn host_skills_key(host: &str) -> String {
    format!("host.{host}.skills")
}

/// Which skills are off per host, kept in memory and in the settings table.
pub struct Enablement {
    /// Where the per-host JSON is kept.
    pub(crate) store: Arc<dyn SettingsRepo>,
    /// The disabled skill names per host.
    pub(crate) disabled: Mutex<BTreeMap<HostId, BTreeSet<SkillName>>>,
    /// Held for the whole of one update, across the store write.
    pub(crate) updates: tokio::sync::Mutex<()>,
}

/// Locks a disabled-skill map without panicking on a poisoned lock.
fn lock(
    map: &Mutex<BTreeMap<HostId, BTreeSet<SkillName>>>,
) -> MutexGuard<'_, BTreeMap<HostId, BTreeSet<SkillName>>> {
    map.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Enablement {
    /// Reads `host.<name>.skills` for each host. Names that are not valid skill names are
    /// ignored, so a corrupt value cannot keep the hub from starting.
    pub async fn load(
        store: Arc<dyn SettingsRepo>,
        hosts: &[Host],
    ) -> Result<Enablement, StoreError> {
        let mut disabled = BTreeMap::<HostId, BTreeSet<SkillName>>::new();
        let empty: Vec<Value> = Vec::new();
        for host in hosts {
            let saved = store.get(&host_skills_key(host.name.as_str())).await?;
            let names = saved
                .as_ref()
                .and_then(|value| value.get("disabled").and_then(Value::as_array))
                .unwrap_or(&empty);
            let set = names
                .iter()
                .filter_map(|name| name.as_str().and_then(|text| SkillName::parse(text).ok()))
                .collect::<BTreeSet<SkillName>>();
            if !set.is_empty() {
                disabled.insert(host.id, set);
            }
        }
        Ok(Self {
            store,
            disabled: Mutex::new(disabled),
            updates: tokio::sync::Mutex::new(()),
        })
    }

    /// Whether the hub must not use `skill` on `host`.
    #[must_use]
    pub fn is_disabled(&self, host: HostId, skill: &SkillName) -> bool {
        let hosts = lock(&self.disabled);
        let Some(set) = hosts.get(&host) else {
            return false;
        };
        set.contains(skill)
    }

    /// Turns `skill` on or off for `host`, in memory and in the settings table. One update
    /// is one whole step: the new names are worked out, saved, and only then put in memory,
    /// so a store that refuses the write leaves the enablement as it was. The update lock is
    /// held the whole time, which keeps two switches on one host from overwriting each
    /// other.
    pub async fn set(
        &self,
        host: &Host,
        skill: SkillName,
        enabled: bool,
    ) -> Result<(), StoreError> {
        let _writing = self.updates.lock().await;
        let names = self.disabled_names(&host.id, skill, enabled);
        let value = json!({
            "disabled": names.iter().map(|name| name.as_str().to_owned()).collect::<Vec<String>>(),
        });
        self.store
            .set(&host_skills_key(host.name.as_str()), &value)
            .await?;
        self.remember(host.id, &names);
        Ok(())
    }

    /// The host's disabled skills with `skill` switched; memory is left alone.
    #[must_use]
    fn disabled_names(
        &self,
        host: &HostId,
        skill: SkillName,
        enabled: bool,
    ) -> BTreeSet<SkillName> {
        let hosts = lock(&self.disabled);
        let mut names = match hosts.get(host) {
            Some(saved) => saved.clone(),
            None => BTreeSet::new(),
        };
        if enabled {
            names.remove(&skill);
        } else {
            names.insert(skill);
        }
        names
    }

    /// Records `names` as `host`'s disabled skills. A host with none drops out.
    fn remember(&self, host: HostId, names: &BTreeSet<SkillName>) {
        let mut hosts = lock(&self.disabled);
        if names.is_empty() {
            hosts.remove(&host);
            return;
        }
        hosts.insert(host, names.clone());
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use domovoi_core::ports::{AgentLink, LinkError, Presence, SettingsRepo, StoreError};
    use domovoi_core::{HostId, ReadToolCall, Risk, ScriptDigest, SkillName, SkillSummary};
    use domovoi_testkit::build::{host, manifest};
    use domovoi_testkit::{FakeAgentLink, InMemoryStore};
    use jiff::Timestamp;
    use serde_json::json;

    use super::{DISABLED_REASON, Enablement, host_skills_key};

    const HIDDEN: &str = "prune_logs";
    const VISIBLE: &str = "zpool_status";

    /// A fixed time for deadlines; the fake agents never look at it.
    fn at(text: &str) -> Timestamp {
        text.parse().unwrap()
    }

    fn named(name: &str) -> SkillName {
        SkillName::parse(name).unwrap()
    }

    /// The summary of a read skill named `name`, as an agent would announce it.
    fn summary(name: &str) -> SkillSummary {
        manifest(name, Risk::Read)
            .unwrap()
            .summary(ScriptDigest::of_bytes(b"fixture entry"))
    }

    /// A connected fake agent on `nas` that announced both skills, and the enablement
    /// with `prune_logs` disabled and saved in a store the tests can read back. The
    /// enablement and the fake are both returned, so a test can compare what the agent
    /// has with what the hub's data says about it.
    async fn with_hidden() -> (
        Enablement,
        HostId,
        Arc<FakeAgentLink>,
        Arc<dyn SettingsRepo>,
    ) {
        let nas = host("nas").unwrap();
        let agent = Arc::new(FakeAgentLink::new());
        agent.set_presence(
            nas.id,
            Presence::Connected {
                since: at("2026-11-01T00:00:00Z"),
                version: "domovoi-agent 0.1".to_owned(),
            },
        );
        agent.set_skills(nas.id, vec![summary(HIDDEN), summary(VISIBLE)]);
        let store: Arc<dyn SettingsRepo> = Arc::new(InMemoryStore::new());
        let policy = Enablement::load(store.clone(), std::slice::from_ref(&nas))
            .await
            .unwrap();
        policy.set(&nas, named(HIDDEN), false).await.unwrap();
        (policy, nas.id, agent, store)
    }

    /// The names `agent` has on `host`, as the hub would list them.
    fn listed(agent: Arc<FakeAgentLink>, host: HostId) -> Vec<String> {
        let announced = agent.skills(host);
        announced
            .iter()
            .map(|item| item.name.as_str().to_owned())
            .collect::<Vec<String>>()
    }

    /// The names a boundary would run: `listed` without the ones `policy` has off.
    fn offered(policy: Enablement, names: &[String], host: HostId) -> Vec<String> {
        names
            .iter()
            .filter(|name| {
                let skill = named(name.as_str());
                !policy.is_disabled(host, &skill)
            })
            .cloned()
            .collect::<Vec<String>>()
    }

    #[tokio::test]
    async fn disabled_skill_hidden_from_skills() {
        let (policy, nas, agent, store) = with_hidden().await;
        let announced = listed(agent, nas);
        assert_eq!(
            announced,
            vec![HIDDEN.to_owned(), VISIBLE.to_owned()],
            "the hub took a name off the agent's own list: {announced:?}",
        );
        let kept = offered(policy, &announced[..], nas);
        assert_eq!(
            kept,
            vec![VISIBLE.to_owned()],
            "the hub hides more than the owner switched off: {kept:?}",
        );
        let saved = store.get(&host_skills_key("nas")).await.unwrap();
        assert_eq!(
            saved,
            Some(json!({ "disabled": [HIDDEN] })),
            "the switch the hub hid is not the switch saved",
        );
    }

    #[tokio::test]
    async fn disabled_skill_handoff_refused() {
        let (policy, nas, _, _) = with_hidden().await;
        assert!(
            policy.is_disabled(nas, &named(HIDDEN)),
            "the handoff boundary would run {HIDDEN}",
        );
        assert!(
            !policy.is_disabled(nas, &named(VISIBLE)),
            "the handoff boundary refuses an enabled skill",
        );
    }

    #[tokio::test]
    async fn disabled_skill_read_skill_tool_refused() {
        let (policy, nas, agent, _) = with_hidden().await;
        let call = ReadToolCall::RunReadSkill {
            skill: named(HIDDEN),
            args: BTreeMap::new(),
        };
        let out = agent.run_tool(nas, call, at("2026-11-01T00:05:00Z")).await;
        assert_eq!(
            agent.requests().len(),
            1,
            "the raw link swallowed a call the engine must refuse",
        );
        assert!(
            !matches!(out, Err(LinkError::Rejected(reason)) if reason == DISABLED_REASON),
            "the link itself refused with the hub's words, so a wrapper is still in place",
        );
        assert!(
            policy.is_disabled(nas, &named(HIDDEN)),
            "the read boundary has nothing to refuse {HIDDEN} with",
        );
        assert_eq!(
            DISABLED_REASON, "the skill is disabled on this host",
            "the refusal the owner sees changed",
        );
    }

    #[tokio::test]
    async fn unwrapped_link_forwards_enabled_calls_to_the_agent() {
        let (policy, nas, agent, _) = with_hidden().await;
        assert!(
            matches!(agent.presence(nas), Presence::Connected { .. }),
            "the raw link lost presence"
        );
        let call = ReadToolCall::RunReadSkill {
            skill: named(VISIBLE),
            args: BTreeMap::new(),
        };
        let out = agent.run_tool(nas, call, at("2026-11-01T00:05:00Z")).await;
        assert_eq!(
            agent.requests().len(),
            1,
            "the enabled call did not reach the agent",
        );
        assert!(
            !matches!(out, Err(LinkError::Rejected(reason)) if reason == DISABLED_REASON),
            "the hub refused a skill it had enabled",
        );
        assert!(
            !policy.is_disabled(nas, &named(VISIBLE)),
            "the enabled call was hidden by the enablement",
        );
    }

    #[tokio::test]
    async fn skill_disable_and_enable_survive_a_reload() {
        let nas = host("nas").unwrap();
        let store: Arc<dyn SettingsRepo> = Arc::new(InMemoryStore::new());
        let policy = Arc::new(
            Enablement::load(store.clone(), std::slice::from_ref(&nas))
                .await
                .unwrap(),
        );
        policy.set(&nas, named(HIDDEN), false).await.unwrap();
        let reloaded = Enablement::load(store.clone(), std::slice::from_ref(&nas))
            .await
            .unwrap();
        assert!(
            reloaded.is_disabled(nas.id, &named(HIDDEN)),
            "a saved disable did not survive the reload"
        );
        policy.set(&nas, named(HIDDEN), true).await.unwrap();
        let revived = Enablement::load(store, std::slice::from_ref(&nas))
            .await
            .unwrap();
        assert!(
            !revived.is_disabled(nas.id, &named(HIDDEN)),
            "a saved enable did not survive the reload",
        );
    }

    #[tokio::test]
    async fn two_switches_on_one_host_keep_both_names() {
        let nas = host("nas").unwrap();
        let store: Arc<dyn SettingsRepo> = Arc::new(InMemoryStore::new());
        let policy = Arc::new(
            Enablement::load(store.clone(), std::slice::from_ref(&nas))
                .await
                .unwrap(),
        );
        let (first, second) = tokio::join!(
            tokio::spawn({
                let policy = policy.clone();
                let other = nas.clone();
                async move { policy.set(&other, named(HIDDEN), false).await }
            }),
            tokio::spawn({
                let policy = policy.clone();
                let other = nas.clone();
                async move { policy.set(&other, named(VISIBLE), false).await }
            }),
        );
        first.unwrap().unwrap();
        second.unwrap().unwrap();
        let saved = store.get(&host_skills_key("nas")).await.unwrap();
        assert_eq!(
            saved,
            Some(json!({ "disabled": [HIDDEN, VISIBLE] })),
            "one of the switches overwrote the other",
        );
    }

    #[tokio::test]
    async fn a_store_that_refuses_the_write_leaves_the_enablement_unchanged() {
        let nas = host("nas").unwrap();
        let fresh = Arc::new(InMemoryStore::new());
        let store: Arc<dyn SettingsRepo> = fresh.clone();
        let policy = Arc::new(
            Enablement::load(store.clone(), std::slice::from_ref(&nas))
                .await
                .unwrap(),
        );
        fresh.fail_next(StoreError::Backend("read-only disk".to_owned()));
        let refused = policy.set(&nas, named(HIDDEN), false).await;
        assert_eq!(
            refused,
            Err(StoreError::Backend("read-only disk".to_owned())),
            "the store refused the write",
        );
        assert!(
            !policy.is_disabled(nas.id, &named(HIDDEN)),
            "the enablement in memory kept the switch the store refused",
        );
        assert_eq!(
            store.get(&host_skills_key("nas")).await.unwrap(),
            None,
            "nothing was saved",
        );
    }
}
