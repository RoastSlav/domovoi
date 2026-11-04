# 14 · Feature: enable and disable skills per host from the web UI

## Goal

On the Skills page each skill has an **Enabled** switch per host. A disabled skill is
still listed (greyed, "Disabled"), but triage does not see it, cannot run it as a read
tool, cannot propose it, and an approved action for it is refused at dispatch.

This is a hub-side policy: it only ever *narrows* what the agent allows, so it needs no
agent or protocol change. (Deleting a skill from the host is prompt 15.)

## What is already known (verified)

- Everything in the engine reaches agents through the `AgentLink` port
  (`crates/domovoi-core/src/ports/agent_link.rs`): `run_tool`, `run_skill`, `run_check`,
  `presence`, `skills`. Triage reads `link.skills(host)` for its catalogue and for
  `run_read_skill` checks (`crates/domovoi-engine/src/triage/{tools,proposals}.rs`).
- The hub passes `server.link()` to two engine services in
  `crates/domovoi-hub/src/runtime/mod.rs`: the action service (`ActionDeps.link`,
  around line 222) and triage (`TriageDeps.link`, around line 414).
- The API lists skills through `AgentAdmin::skills` (`crates/domovoi-hub/src/api/mod.rs`),
  which must keep showing disabled skills.
- Per-host settings are stored with `SettingsRepo` (`get`/`set` JSON by key); check
  overrides use the key `host.<name>.checks` (`crates/domovoi-hub/src/settings/stored.rs`,
  `host_checks_key`). Use `host.<name>.skills` the same way.
- `runtime/mod.rs` is 432 lines; keep additions there to a few lines and put new code in
  a new file.

## Read these files

- `crates/domovoi-core/src/ports/agent_link.rs`
- `crates/domovoi-hub/src/runtime/mod.rs` (only the two `link:` places and where `store`
  is opened)
- `crates/domovoi-hub/src/settings/stored.rs` (`host_checks_key`)
- `crates/domovoi-hub/src/api/{mod,hosts,settings_write}.rs`
- `web/src/features/skills/SkillList.tsx`, `web/src/api/hooks/hosts.ts`

## Steps

1. New file `crates/domovoi-hub/src/agent_server/skill_policy.rs`:
   - `pub struct SkillPolicy` holding
     `std::sync::RwLock<BTreeMap<HostId, BTreeSet<SkillName>>>` and the store
     (`Arc<dyn SettingsRepo>`), with:
     `pub async fn load(store, hosts: &[Host]) -> Result<Self, StoreError>` (reads
     `host.<name>.skills` → `{"disabled": ["name", ..]}` for each host),
     `pub fn is_disabled(&self, host: HostId, skill: &SkillName) -> bool`,
     `pub async fn set(&self, host: &Host, skill: SkillName, enabled: bool) -> Result<(), StoreError>`
     (updates memory and saves the JSON).
   - `pub struct PolicyLink { inner: Arc<dyn AgentLink>, policy: Arc<SkillPolicy> }`
     implementing `AgentLink` by delegation, except: `skills()` drops disabled skills;
     `run_skill()` returns `LinkError::Rejected("the skill is disabled on this host")`
     when `req`'s skill is disabled; `run_tool()` does the same for
     `ReadToolCall::RunReadSkill { skill, .. }`.
   Declare the module in `agent_server/mod.rs` and re-export both types.
2. `runtime/mod.rs`: after the store is open, build
   `let skill_policy = Arc::new(SkillPolicy::load(store.clone(), &store.list_hosts().await?).await?);`
   and pass `Arc::new(PolicyLink { inner: server.link(), policy: skill_policy.clone() })`
   to both `ActionDeps.link` and `TriageDeps.link` (the triage builder takes `server`;
   add a `link: Arc<dyn AgentLink>` field to `TriageBuild` or pass the wrapped link in).
   Add `pub skill_policy: Arc<SkillPolicy>` to `ApiDeps` and fill it in the three
   construction sites (`crates/domovoi-hub/src/runtime/mod.rs`,
   `crates/domovoi-hub/examples/dev_hub.rs`, `crates/domovoi-hub/tests/hub/api_harness.rs`).
3. API: add `enabled: bool` to `SkillDto` (from `skill_policy.is_disabled`). Add
   `PUT /api/v1/hosts/{name}/skills/{skill}` with body `{ "enabled": bool }` → 204,
   audited as `skill.enabled` / `skill.disabled` (`actor: user:<id>`,
   `subject: host:<name>/skill:<skill>`). Add it to the `writes` list in
   `crates/domovoi-hub/tests/gates.rs`. Regenerate OpenAPI.
4. Web: `useSetSkillEnabled(host)` hook that invalidates `keys.skills(host)`. In
   `SkillCard` add a labelled checkbox switch "Enabled"; a disabled card gets
   `data-disabled="true"` (CSS: opacity 0.6) and a "Disabled" tag. Explain under the
   page intro: "Disabled skills stay on the host but Domovoi never runs or suggests
   them."

## Tests

- Hub unit tests for `PolicyLink` with `domovoi_testkit::FakeAgentLink`: a disabled skill
  is missing from `skills()`, `run_skill` and `run_tool(RunReadSkill)` are rejected,
  other calls pass through.
- Hub API test: disable → list shows `enabled: false` → enable again. Settings survive a
  new `SkillPolicy::load`.
- Engine is unchanged; existing tests must pass.
- Web: toggling sends `PUT` with `{ enabled: false }`.

## Done when

- Disabled skills are invisible to triage and refused at dispatch, visible and
  re-enableable in the UI, and survive a hub restart.
- All tests pass; `runtime/mod.rs` stays under 500 lines.
