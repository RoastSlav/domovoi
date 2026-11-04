# 15 · Feature: add and delete skills from the web UI

Three prompts, run in order. **15b changes a security invariant (INV-3: "the hub cannot
push scripts").** Decide before starting whether you want that. 15a and the "copy
commands" part of 15c are useful on their own and change no invariant.

Do prompt 14 (enable/disable) first; it covers "turn a skill off" without any of this.

## Background (verified, applies to all three)

- Skills live on the agent host in `<skills_dir>/<name>/` (`skill.toml`, an executable
  entry, optional `README.md`), default `/etc/domovoi/skills`
  (`crates/domovoi-config/src/agent.rs`, `AgentSection.skills_dir`).
- The agent loads them **once at start** (`crates/domovoi-agent/src/runtime.rs`,
  `load_catalog`) into `RequestCtx.catalog: Arc<SkillCatalog>`; uses are in
  `runtime.rs:193` and `requests/skills.rs` (4 places). `SkillCatalog::reload()` already
  exists in `crates/domovoi-runner/src/catalog/mod.rs`.
- The hub learns skills only from `Hello.skills` and stores them per live connection
  (`crates/domovoi-hub/src/agent_server/registry.rs`, field `skills`, accessor
  `skills(host)`). Incoming agent messages are handled in
  `crates/domovoi-hub/src/agent_server/session.rs`, `handle()`.
- The agent turns hub messages into requests in `crates/domovoi-agent/src/link/mod.rs`
  (around line 290: `Message::RunTool(run) => RequestKind::Tool(run)`), and serves them in
  `crates/domovoi-agent/src/requests/mod.rs`.
- Catalogue rules (enforced by the runner on load): manifest parses and validates, name
  equals folder name, folder not world-writable, entry is a regular executable file in
  the folder, not group- or world-writable.

---

## 15a · The agent picks up skill changes without a restart

### Steps

1. Agent: change `RequestCtx.catalog` to
   `catalog: Arc<std::sync::RwLock<Arc<SkillCatalog>>>` and add
   `pub fn catalog(&self) -> Arc<SkillCatalog>` (read lock, clone the `Arc`; recover a
   poisoned lock with `PoisonError::into_inner`). Update the four uses in
   `requests/skills.rs` to `let catalog = ctx.catalog();` then `catalog.get(..)`.
2. Agent: in `runtime.rs` `run()`, spawn a task that every 60 s (use
   `deps.clock.sleep_until(now + 60 s)` in a loop with the shutdown token) clones the
   current catalogue, calls `reload()`, logs each `CatalogIssue` as a warning, and when
   `summaries()` differ from the current ones, swaps the new catalogue in and sends
   `Message::SkillsUpdate(SkillsUpdate { skills })` through the `LinkHandle`.
   Also make the `Hello` built on every reconnect use the current summaries (today
   `LinkParts.skills` is a fixed `Vec`; replace it with a shared
   `Arc<std::sync::RwLock<Vec<SkillSummary>>>` that the reload task updates).
3. Proto: add `SkillsUpdate { skills: Vec<SkillSummary> }` = `"skills_update"`
   (agent → hub) in `crates/domovoi-proto/src/messages/stream.rs` and the `messages!`
   list; sample + snapshot; protocol minor per rule 10.
4. Hub: in `session.rs` `handle()`, on `Message::SkillsUpdate(update)`, call a new
   `registry.set_skills(host, session, update.skills)` that replaces the live entry's
   skills. Log "skills updated" with the count.
5. Docs: `docs/book/src/guide/skills.md` — "The agent reads the folder when it starts"
   becomes "The agent re-reads the folder every minute; no restart needed."

### Tests

- Agent: write a skill into a temp skills dir after start, advance the fake clock 61 s,
  and assert a `skills_update` message with the new skill was sent (see
  `crates/domovoi-agent/tests/agent/runtime.rs` for a runtime harness).
- Hub: a `skills_update` replaces what `skills(host)` returns.

---

## 15b · Opt-in: the hub may install and remove skills (security change)

### Steps

1. **ADR.** Add `docs/adr/0018-hub-managed-skills.md` (copy `0000-template.md`).
   Decision: an agent accepts skills from the hub only when its own `agent.toml` says
   `[skills] manage_from_hub = true` (default `false`). Hub-managed skills live in a
   separate directory, `hub_skills_dir` (default `<state_dir>/hub-skills`), are loaded by
   the same catalogue rules, may not shadow a local skill of the same name, and may only
   use the risks in `[skills] hub_allow_risk` (default `["read"]`), on top of
   `[policy] allow_risk`. Local skills in `skills_dir` are never written or removed by
   the hub. Update INV-3 in `ARCHITECTURE.md` §15 and §12 to say exactly this.
2. **Config.** Add `SkillsSection { manage_from_hub: bool, hub_skills_dir: Option<PathBuf>, hub_allow_risk: Vec<Risk> }`
   as `[skills]` in `AgentConfig` (with `#[serde(default)]`, docs on every field,
   `deny_unknown_fields`). Update `examples/agent.toml` with a commented block.
3. **Proto** (`messages/work.rs`): `InstallSkill { name: SkillName, manifest_toml: String, entry: String, readme: Option<String> }`
   = `"install_skill"`, `RemoveSkill { name: SkillName }` = `"remove_skill"`, and the reply
   `SkillChange { ok: bool, issues: Vec<String> }` = `"skill_change"`. Samples, snapshots,
   protocol minor. When opted in, the agent adds `"hub_skills"` to `Hello.capabilities`.
4. **Agent handler** (new `crates/domovoi-agent/src/requests/manage.rs`):
   - Refuse unless `manage_from_hub`; refuse names that exist in the local catalogue.
   - Limits: manifest ≤ 16 KiB, entry ≤ 64 KiB, README ≤ 16 KiB; the entry must start
     with `#!`.
   - Write into a fresh temp folder inside `hub_skills_dir`: `skill.toml`, `run`
     (mode `0o755`), `README.md`; the manifest's `entry` must be `"run"`. Load that folder
     with the runner's catalogue checks; check `risk` is in `hub_allow_risk`. On any
     problem delete the temp folder and reply `ok: false` with the issues. Otherwise
     rename it to `<hub_skills_dir>/<name>` (replace an existing hub-managed one
     atomically: rename old → `.old`, new → name, delete `.old`).
   - Remove deletes `<hub_skills_dir>/<name>` only (never `skills_dir`).
   - After either, reload (15a) so a `skills_update` goes out.
   - The agent's catalogue now loads both directories; expose that from the runner as
     `SkillCatalog::load_many(&[dir1, dir2])` or load two catalogues and merge in the
     agent, local first.
   File writes are allowed in the agent (it is an adapter crate); never spawn a process.
5. **Hub link.** Add inherent methods on the hub's `WsAgentLink`
   (`crates/domovoi-hub/src/agent_server/link.rs`) `install_skill(..)` and
   `remove_skill(..)` using its private `request()` helper with a 15 s deadline; map an
   `unsupported` reply to "this agent is too old to manage skills".

### Tests

- Agent: install refused when not opted in; refused for a bad manifest (temp folder
  gone afterwards); refused for `risk = "write"` with the default `hub_allow_risk`;
  accepted for a valid read skill (folder exists, mode 755, catalogue lists it); remove
  deletes only the hub-managed folder; a name clash with a local skill is refused.
- Gate: add a test to `crates/domovoi-agent/tests/gates.rs` named
  `inv3_hub_cannot_write_skills_unless_the_agent_opts_in`.

---

## 15c · Web UI: add, edit and delete skills

### Steps

1. **API** (new `crates/domovoi-hub/src/api/skills_manage.rs`):
   - `POST /api/v1/skills/validate` with `{ manifest_toml }` → `{ issues: [..] }`, parsing
     with `toml` + `SkillManifest::validate` (add `toml` to `domovoi-hub` from
     `[workspace.dependencies]`). No agent involved.
   - `POST /api/v1/hosts/{name}/skills` with `{ manifest_toml, entry, readme, password }`
     and `DELETE /api/v1/hosts/{name}/skills/{skill}` with `{ password }`. Both re-check
     the admin password the way sensitive settings saves do (find the password check in
     `crates/domovoi-hub/src/api/settings_write.rs` / `settings/service.rs` and reuse
     it), call 15b's link methods, audit `skill.installed` / `skill.removed` with the
     manifest hash, and return the agent's issues as 400. Add both to the `writes` list
     in `crates/domovoi-hub/tests/gates.rs`. Regenerate OpenAPI.
   - Add `managed_by_hub: bool` to `SkillDto` (true when the skill came from
     `hub_skills_dir`; carry it as an optional `source` field in `SkillSummary`, default
     `"local"`), and `hub_skills: bool` to `HostDto` (from the agent's capabilities).
2. **Web:** on the Skills page, an **Add skill** button opens
   `web/src/features/skills/SkillEditor.tsx`: name, description, risk (read only, unless
   the host allows more), timeout, applies to, a params editor (name, description, kind:
   string with pattern/max_len, integer with min/max, boolean, enum with values), the
   script (monospace textarea, prefilled `#!/bin/sh\nset -u\n`), README. Build the TOML
   on the client, call validate as you type (debounced with `useDebounced`), show issues.
   Save asks for the password with `PasswordPrompt`.
   - When the host has `hub_skills: true`: Save installs it. Hub-managed skills get
     Edit and Delete buttons; local skills show "Installed on the host; edit it there."
   - When it does not: show **Copy install commands** instead: a `CopyBlock` with
     `sudo mkdir -p /etc/domovoi/skills/<name>`, `sudo tee` heredocs for `skill.toml`,
     `run`, `README.md`, `sudo chmod 755 .../run`, and
     `sudo domovoi check skills /etc/domovoi/skills`, plus a note on how to opt in.
3. **Docs:** a "Managing skills from the web UI" section in
   `docs/book/src/guide/skills.md` and the security implications in
   `docs/book/src/security.md`.

### Done when

- On an opted-in agent, a read skill can be created, edited and deleted from the UI,
  each change audited and password-confirmed; it shows up for triage within a minute.
- On other agents the UI produces correct copy-paste commands.
- All gate tests pass, including the new INV-3 one.
