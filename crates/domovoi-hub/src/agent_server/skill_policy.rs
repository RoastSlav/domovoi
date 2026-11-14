//! Which skills the hub may use on each host: the UI's per-host switch, saved in the
//! settings table under `host.<name>.skills` as `{"disabled": ["name", ..]}`. The policy
//! only ever *narrows* what the agent itself allows, so nothing on the agent or the wire
//! changes: [`PolicyLink`] wraps the real link and hides the disabled skills from
//! everything the engine asks, refusing them at dispatch too.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use domovoi_core::ports::{
    AgentLink, CheckRecheck, HostAvailability, LinkError, Presence, SettingsRepo, SkillDispatch,
    SkillOutput, StoreError, ToolOutput,
};
use domovoi_core::skill::SkillName;
use domovoi_core::{CheckKey, Host, HostId, ReadToolCall, SkillSummary, TargetKey};
use jiff::Timestamp;
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
pub struct SkillPolicy {
    /// Where the per-host JSON is kept.
    pub(crate) store: Arc<dyn SettingsRepo>,
    /// The disabled skill names per host.
    pub(crate) disabled: Mutex<BTreeMap<HostId, BTreeSet<SkillName>>>,
    /// Held for the whole of one update, across the store write.
    pub(crate) updates: tokio::sync::Mutex<()>,
}

/// Locks a policy map without panicking on a poisoned lock.
fn lock(
    map: &Mutex<BTreeMap<HostId, BTreeSet<SkillName>>>,
) -> MutexGuard<'_, BTreeMap<HostId, BTreeSet<SkillName>>> {
    map.lock().unwrap_or_else(PoisonError::into_inner)
}

impl SkillPolicy {
    /// Reads `host.<name>.skills` for each host. Names that are not valid skill names are
    /// ignored, so a corrupt value cannot keep the hub from starting.
    pub async fn load(
        store: Arc<dyn SettingsRepo>,
        hosts: &[Host],
    ) -> Result<SkillPolicy, StoreError> {
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
    /// so a store that refuses the write leaves the policy as it was. The update lock is
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

/// The link the engine uses: the real link with the per-host policy applied.
pub struct PolicyLink {
    /// The link to the real agents.
    pub(crate) inner: Arc<dyn AgentLink>,
    /// The per-host policy applied to every request.
    pub(crate) policy: Arc<SkillPolicy>,
}

impl PolicyLink {
    /// Wraps `inner` so every request to the agents goes through `policy`.
    #[must_use]
    pub fn wrap(inner: Arc<dyn AgentLink>, policy: Arc<SkillPolicy>) -> PolicyLink {
        PolicyLink { inner, policy }
    }
}

#[async_trait]
impl AgentLink for PolicyLink {
    async fn run_tool(
        &self,
        host: HostId,
        call: ReadToolCall,
        deadline: Timestamp,
    ) -> Result<ToolOutput, LinkError> {
        if let ReadToolCall::RunReadSkill { skill, .. } = &call
            && self.policy.is_disabled(host, skill)
        {
            return Err(LinkError::Rejected(DISABLED_REASON.to_owned()));
        }
        AgentLink::run_tool(&*self.inner, host, call, deadline).await
    }

    async fn run_skill(
        &self,
        host: HostId,
        req: SkillDispatch,
        deadline: Timestamp,
    ) -> Result<SkillOutput, LinkError> {
        if self.policy.is_disabled(host, &req.skill) {
            return Err(LinkError::Rejected(DISABLED_REASON.to_owned()));
        }
        AgentLink::run_skill(&*self.inner, host, req, deadline).await
    }

    async fn run_check(
        &self,
        host: HostId,
        check: CheckKey,
        target: Option<TargetKey>,
        deadline: Timestamp,
    ) -> Result<CheckRecheck, LinkError> {
        AgentLink::run_check(&*self.inner, host, check, target, deadline).await
    }

    fn presence(&self, host: HostId) -> Presence {
        AgentLink::presence(&*self.inner, host)
    }

    async fn availability(&self, host: HostId) -> HostAvailability {
        AgentLink::availability(&*self.inner, host).await
    }

    fn skills(&self, host: HostId) -> Vec<SkillSummary> {
        let inner = &*self.inner;
        AgentLink::skills(inner, host)
            .iter()
            .filter(|skill| !self.policy.is_disabled(host, &skill.name))
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use domovoi_core::ports::{
        AgentLink, LinkError, Presence, SettingsRepo, SkillDispatch, StoreError,
    };
    use domovoi_core::{ActionId, HostId, PlanHash, ReadToolCall, Risk, SkillName, SkillSummary};
    use domovoi_testkit::build::{host, manifest};
    use domovoi_testkit::{FakeAgentLink, InMemoryStore};
    use jiff::Timestamp;
    use serde_json::json;

    use super::{DISABLED_REASON, PolicyLink, SkillPolicy, host_skills_key};

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
        manifest(name, Risk::Read).unwrap().summary()
    }

    /// A dispatch for skill `name`, as the action executor would build one.
    fn dispatch(name: &str, id: HostId) -> SkillDispatch {
        let skill = named(name);
        let built = manifest(skill.as_str(), Risk::Write).unwrap();
        let args = built.validate_args(&BTreeMap::new()).unwrap();
        SkillDispatch {
            action: ActionId::new(),
            plan_hash: PlanHash::compute(&skill, built.manifest_hash(), id, &args),
            skill,
            args,
        }
    }

    /// A [`PolicyLink`] over a connected fake agent on `nas` that announced both skills,
    /// with `prune_logs` disabled in the policy and saved in the settings store. Also
    /// returns the fake, so tests can check which calls actually reached it.
    async fn wrapped() -> (PolicyLink, HostId, Arc<FakeAgentLink>) {
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
        let policy = Arc::new(
            SkillPolicy::load(store.clone(), std::slice::from_ref(&nas))
                .await
                .unwrap(),
        );
        policy.set(&nas, named(HIDDEN), false).await.unwrap();
        let link = PolicyLink::wrap(agent.clone(), policy);
        (link, nas.id, agent)
    }

    #[tokio::test]
    async fn disabled_skill_hidden_from_skills() {
        let (link, nas, agent) = wrapped().await;
        let listed = link.skills(nas);
        let seen = listed
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<&str>>();
        assert_eq!(seen, vec![VISIBLE], "{seen:?}");
        let announced = agent.skills(nas);
        assert_eq!(
            announced
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<&str>>(),
            vec![HIDDEN, VISIBLE],
            "the agent still has the skill, only the policy hides it",
        );
    }

    #[tokio::test]
    async fn disabled_skill_run_skill_refused() {
        let (link, nas, agent) = wrapped().await;
        let out = link
            .run_skill(nas, dispatch(HIDDEN, nas), at("2026-11-01T00:05:00Z"))
            .await;
        match out {
            Err(LinkError::Rejected(reason)) => assert_eq!(reason, DISABLED_REASON),
            Err(other) => panic!("wrong reason for a refused skill: {other:?}"),
            Ok(out) => panic!("a disabled skill ran: {out:?}"),
        }
        assert!(
            agent.requests().is_empty(),
            "the policy let the skill reach the agent"
        );
    }

    #[tokio::test]
    async fn disabled_skill_read_skill_tool_refused() {
        let (link, nas, agent) = wrapped().await;
        let call = ReadToolCall::RunReadSkill {
            skill: named(HIDDEN),
            args: BTreeMap::new(),
        };
        let out = link.run_tool(nas, call, at("2026-11-01T00:05:00Z")).await;
        match out {
            Err(LinkError::Rejected(reason)) => assert_eq!(reason, DISABLED_REASON),
            Err(other) => panic!("wrong reason for a refused tool call: {other:?}"),
            Ok(out) => panic!("a disabled skill ran as a read tool: {out:?}"),
        }
        assert!(
            agent.requests().is_empty(),
            "the policy let the call reach the agent"
        );
    }

    #[tokio::test]
    async fn policy_link_forwards_other_calls_to_the_agent() {
        let (link, nas, agent) = wrapped().await;
        assert!(
            matches!(link.presence(nas), Presence::Connected { .. }),
            "the link lost presence"
        );
        let call = ReadToolCall::RunReadSkill {
            skill: named(VISIBLE),
            args: BTreeMap::new(),
        };
        let out = link.run_tool(nas, call, at("2026-11-01T00:05:00Z")).await;
        assert_eq!(
            agent.requests().len(),
            1,
            "the enabled call did not reach the agent"
        );
        assert!(
            matches!(out, Err(LinkError::Rejected(why)) if why != DISABLED_REASON),
            "the policy blocked an enabled skill",
        );
    }

    #[tokio::test]
    async fn skill_disable_and_enable_survive_a_reload() {
        let nas = host("nas").unwrap();
        let store: Arc<dyn SettingsRepo> = Arc::new(InMemoryStore::new());
        let policy = Arc::new(
            SkillPolicy::load(store.clone(), std::slice::from_ref(&nas))
                .await
                .unwrap(),
        );
        policy.set(&nas, named(HIDDEN), false).await.unwrap();
        let reloaded = SkillPolicy::load(store.clone(), std::slice::from_ref(&nas))
            .await
            .unwrap();
        assert!(
            reloaded.is_disabled(nas.id, &named(HIDDEN)),
            "a saved disable did not survive the reload"
        );
        policy.set(&nas, named(HIDDEN), true).await.unwrap();
        let revived = SkillPolicy::load(store, std::slice::from_ref(&nas))
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
            SkillPolicy::load(store.clone(), std::slice::from_ref(&nas))
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
    async fn a_store_that_refuses_the_write_leaves_the_policy_unchanged() {
        let nas = host("nas").unwrap();
        let fresh = Arc::new(InMemoryStore::new());
        let store: Arc<dyn SettingsRepo> = fresh.clone();
        let policy = Arc::new(
            SkillPolicy::load(store.clone(), std::slice::from_ref(&nas))
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
            "the policy in memory kept the switch the store refused",
        );
        assert_eq!(
            store.get(&host_skills_key("nas")).await.unwrap(),
            None,
            "nothing was saved",
        );
    }
}
