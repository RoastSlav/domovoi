# 15 · Feature: add and delete skills from the web UI

Three prompts, run in order. 15a and the "copy commands" part of 15c are useful on their
own and change no invariant. **15b changes INV-3 ("the hub cannot push scripts") and
waits for a decision from the maintainer** (see "Decision before 15b").

Done already: per-host enable and disable (`crates/domovoi-hub/src/agent_server/skill_policy.rs`,
`PUT /api/v1/hosts/{name}/skills/{skill}`), skill details and README in the UI.
Do 32 (per-host settings table) first: 15b and 15c add per-host state.

## Background (verified on `main`)

- Skills live on the agent host in `<skills_dir>/<name>/` (`skill.toml`, an executable
  entry, optional `README.md`), default `/etc/domovoi/skills`
  (`crates/domovoi-config/src/agent.rs:52`, `skills_dir`; `state_dir` at `:50`).
- The agent loads them **once at start**: `runtime.rs:80` `load_catalog`, stored at
  `runtime.rs:138` into `RequestCtx.catalog: Arc<SkillCatalog>`
  (`crates/domovoi-agent/src/requests/mod.rs:69`); used in `requests/skills.rs`.
  `SkillCatalog::reload()` and `summaries()` exist
  (`crates/domovoi-runner/src/catalog/mod.rs:78,101`).
- The hub learns skills only from `Hello.skills` and keeps them per live session
  (`crates/domovoi-hub/src/agent_server/registry.rs`, `skills(host)` at `:263`).
  Incoming agent messages are handled in `agent_server/session.rs` `handle()`.
  `PolicyLink` filters disabled skills on top (`skill_policy.rs`).
- The agent maps hub messages to requests in `crates/domovoi-agent/src/link/mod.rs:295-298`
  and serves them in `requests/mod.rs`.
- `Hello.capabilities: Vec<String>` exists (`crates/domovoi-proto/src/messages/handshake.rs:164`).
- Catalogue rules (runner, on load): manifest parses and validates, name equals folder
  name, folder not world-writable, entry is a regular executable file in the folder, not
  group- or world-writable.
- The book says today: "to stop one from running, delete its folder and restart the
  agent" (`docs/book/src/guide/skills.md:27`).
- ADR numbers 0019 and 0020 are taken by prompts 25 and 31; use the next free one.

---

## 15a · The agent picks up skill changes without a restart

### Steps

1. Agent: change `RequestCtx.catalog` to
   `Arc<std::sync::RwLock<Arc<SkillCatalog>>>` and add
   `pub fn catalog(&self) -> Arc<SkillCatalog>` (read lock, clone the `Arc`; recover a
   poisoned lock with `PoisonError::into_inner`). Update every use in
   `requests/skills.rs` and `runtime.rs`.
2. Agent: in `runtime.rs` `run()`, spawn a task that every 60 s (a loop on
   `deps.clock.sleep_until(now + 60 s)` with the shutdown token) clones the current
   catalogue, calls `reload()`, logs each `CatalogIssue` as a warning, and when
   `summaries()` differ, swaps the new catalogue in and sends
   `Message::SkillsUpdate(SkillsUpdate { skills })` through the link handle. The `Hello`
   built on every reconnect uses the current summaries (replace the fixed skills `Vec`
   in the link parts with a shared `Arc<RwLock<Vec<SkillSummary>>>` the reload task
   updates). Put the task in a new `crates/domovoi-agent/src/skills_reload.rs`.
3. Proto: `SkillsUpdate { skills: Vec<SkillSummary> }` = `"skills_update"`
   (agent → hub) in `crates/domovoi-proto/src/messages/stream.rs` and the `messages!`
   list; sample and snapshot; protocol minor per rule 10. An older hub answers
   `unsupported`; the agent logs it once and keeps going.
4. Hub: in `session.rs` `handle()`, on `SkillsUpdate`, call a new
   `registry.set_skills(host, session, skills)` that replaces the live entry's skills,
   and publish whatever event makes the Skills page refresh (check how a new session
   is announced to SSE; reuse it). Log "skills updated" with the count.
5. Book: `docs/book/src/guide/skills.md:27`: "The agent re-reads the folder every
   minute; no restart needed."

### Tests

- Agent: write a skill into a temp skills dir after start, advance the fake clock 61 s,
  assert a `skills_update` with the new skill was sent
  (`crates/domovoi-agent/tests/agent/runtime.rs` has a runtime harness).
- Agent: an unchanged folder sends nothing.
- Hub: a `skills_update` replaces what `skills(host)` returns; a disabled skill stays
  hidden by `PolicyLink`.

---

## Decision before 15b

Letting the hub write scripts to hosts means a stolen hub session, or a compromised hub,
can run code on every opted-in host. The audit recommended, in order of preference:

1. **Skip 15b.** 15c generates copy-paste commands, and 15a makes them take effect
   within a minute. INV-3 stays as it is.
2. **15b as below**, with all of: opt-in in the agent's own config, a separate folder,
   read-only risk by default, the admin password on every install, an audit entry with
   the manifest hash, and the hash shown on the host page.
3. **15b plus signing**: the agent only accepts skills signed with a key that is not on
   the hub (minisign, like prompt 31). Strongest, but the operator signs on their own
   machine, which removes most of the convenience of the web editor.

Write the choice into the ADR. The steps below are option 2.

## 15b · Opt-in: the hub may install and remove skills (security change)

### Steps

1. **ADR** `docs/adr/00NN-hub-managed-skills.md`: an agent accepts skills from the hub
   only when its own `agent.toml` says `[skills] manage_from_hub = true` (default
   `false`). Hub-managed skills live in `hub_skills_dir` (default
   `<state_dir>/hub-skills`), are loaded by the same catalogue rules, may not shadow a
   local skill of the same name, and may only use the risks in
   `[skills] hub_allow_risk` (default `["read"]`), on top of `[policy] allow_risk`. Local
   skills in `skills_dir` are never written or removed by the hub. Update INV-3 in
   `ARCHITECTURE.md` §15 and §12 to say exactly this.
2. **Config.** `SkillsSection { manage_from_hub: bool, hub_skills_dir: Option<PathBuf>, hub_allow_risk: Vec<Risk> }`
   as `[skills]` in `AgentConfig` (`#[serde(default)]`, docs on every field,
   `deny_unknown_fields`). Commented block in `examples/agent.toml`. Run
   `cargo xtask docs-config`.
3. **Proto** (`messages/work.rs`): `InstallSkill { name, manifest_toml, entry, readme: Option<String> }`
   = `"install_skill"`, `RemoveSkill { name }` = `"remove_skill"`, reply
   `SkillChange { ok: bool, issues: Vec<String> }` = `"skill_change"`. Samples,
   snapshots, protocol minor. When opted in, the agent adds `"hub_skills"` to
   `Hello.capabilities`.
4. **Agent handler** (new `crates/domovoi-agent/src/requests/manage.rs`):
   - Refuse unless `manage_from_hub`; refuse names in the local catalogue.
   - Limits: manifest ≤ 16 KiB, entry ≤ 64 KiB, README ≤ 16 KiB; the entry starts with
     `#!`.
   - Write into a fresh temp folder inside `hub_skills_dir`: `skill.toml`, `run`
     (mode `0o755`), `README.md`; the manifest's `entry` must be `"run"`. Load it with
     the runner's catalogue checks; check `risk` is in `hub_allow_risk`. On any problem
     delete the temp folder and reply `ok: false` with the issues. Otherwise rename it
     to `<hub_skills_dir>/<name>` (replace an existing hub-managed one: old → `.old`,
     new → name, delete `.old`).
   - Remove deletes `<hub_skills_dir>/<name>` only, never `skills_dir`.
   - After either, trigger the 15a reload at once so a `skills_update` goes out.
   - The catalogue loads both directories, local first (a `SkillCatalog::load_many`
     in the runner, or two catalogues merged in the agent).
   - File writes are fine in the agent (adapter crate); never spawn a process.
5. **Hub link.** Inherent methods `install_skill(..)` and `remove_skill(..)` on the hub's
   link (`crates/domovoi-hub/src/agent_server/link.rs`) using its `request()` helper
   with a 15 s deadline; an `unsupported` reply becomes "this agent is too old to manage
   skills".

### Tests

- Agent: refused when not opted in; refused for a bad manifest (temp folder gone);
  refused for `risk = "write"` with the default `hub_allow_risk`; accepted for a valid
  read skill (folder exists, mode 755, catalogue lists it); remove deletes only the
  hub-managed folder; a name clash with a local skill is refused.
- Gate in `crates/domovoi-agent/tests/gates.rs`:
  `inv3_hub_cannot_write_skills_unless_the_agent_opts_in`.

---

## 15c · Web UI: add, edit and delete skills

### Steps

1. **API** (new `crates/domovoi-hub/src/api/skills_manage.rs`):
   - `POST /api/v1/skills/validate` `{ manifest_toml }` → `{ issues }`, parsing with
     `toml` + `SkillManifest::validate` (add `toml` to `domovoi-hub` from
     `[workspace.dependencies]`). No agent involved.
   - Only with 15b: `POST /api/v1/hosts/{name}/skills` `{ manifest_toml, entry, readme, password }`
     and `DELETE /api/v1/hosts/{name}/skills/{skill}` `{ password }`. Both re-check the
     admin password the way sensitive settings saves do
     (`crates/domovoi-hub/src/settings/service.rs:232-243`), call 15b's link methods,
     audit `skill.installed` / `skill.removed` with the manifest hash, and return the
     agent's issues as 400. Add both to the `writes` list in
     `crates/domovoi-hub/tests/gates.rs`. Regenerate OpenAPI.
   - `managed_by_hub: bool` on the skill DTO (carry an optional `source` in
     `SkillSummary`, default `"local"`), `hub_skills: bool` on the host DTO (from the
     agent's capabilities).
2. **Web** (follow `web/DESIGN.md`; the Skills page already has a header action slot
   and a row slot for these): the **Add skill** button in the Skills `PageHeader`
   opens `web/src/features/skills/SkillEditor.tsx` (split into sibling files to stay
   under the line limit): `Field`s for name, description, risk (`SegmentedControl` with
   `riskMeta` pills; options the host does not allow are disabled with a
   `disabledReason`), timeout, applies to; a params editor (rows of string with
   pattern/max_len, integer with min/max, boolean, enum with values; add/remove rows);
   the script (monospace `Textarea`, prefilled `#!/bin/sh\nset -u\n`); README. Build the
   TOML on the client, validate as you type (debounced with `useDebounced`), show issues
   per field and in a `FormErrorSummary`, and a live `success` `InlineMessage` "Valid
   skill" when there are none.
   - Host with `hub_skills: true` (15b): Save asks for the password (`PasswordPrompt`)
     and installs, reporting through `useActionFeedback`. Hub-managed skills get a
     "hub-managed" `Tag` and Edit and Delete (`ConfirmDialog` naming the skill and host)
     in the row menu; local skills show Edit disabled with the `disabledReason`
     "Installed on the host; edit it there."
   - Otherwise (or without 15b): **Copy install commands** (primary button): a `CopyBlock` with
     `sudo mkdir -p /etc/domovoi/skills/<name>`, `sudo tee` heredocs for `skill.toml`,
     `run` and `README.md`, `sudo chmod 755 .../run`, and
     `sudo domovoi check skills /etc/domovoi/skills`. With 15a it appears within a
     minute; say so.
3. **Docs:** "Managing skills from the web UI" in `docs/book/src/guide/skills.md`; with
   15b, the security trade-off in `docs/book/src/security.md`.

### Done when

- Without 15b: the editor validates a skill and produces correct commands, and the new
  skill shows up for triage within a minute of running them.
- With 15b: on an opted-in agent a read skill can be created, edited and deleted from the
  UI, each change audited and password-confirmed; all gate tests pass, including the new
  INV-3 one.
