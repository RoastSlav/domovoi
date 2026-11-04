# 24 · Feature: a baseline profile of each machine, gathered on first connect

Three prompts, run in order: 24a (two read-only starter skills), 24b (collect and show
the profile), 24c (triage and chat use it, and changes are tracked).

## Goal

Within a minute of an agent's first connection, the hub gathers a **baseline profile**
of that machine: OS, kernel, CPU, RAM and swap, disks and mounts, Docker containers,
running services, listening ports, the usual log noise, kernel messages, installed
skills and enabled checks. It is saved, shown on the host page, refreshed when the
machine changes and once a week, and given to the model so triage knows what is normal
on that host.

## Background (verified, applies to all three)

- The agent's `Hello` carries `HostFacts` (`crates/domovoi-proto/src/messages/handshake.rs`):
  hostname, os, kernel, arch, cpu_count, mem_total_bytes, boot_time, boot_id. **The hub
  throws them away**: `crates/domovoi-hub/src/agent_server/session.rs` (around line 30)
  builds the registry's `Live { session, since, version, skills, checks, .. }` without
  them (`crates/domovoi-hub/src/agent_server/registry.rs`, `struct Live`).
- The hub can already ask any connected agent for read-only data with the `AgentLink`
  port (`crates/domovoi-core/src/ports/agent_link.rs`, `run_tool(host, ReadToolCall, deadline)`);
  `AgentServer::link()` returns it. Useful calls (`crates/domovoi-core/src/triage/tool_call.rs`):
  - `HostOverview`: uptime, load, memory, swap, mounts with usage (text).
  - `DockerPs { all: true }`: TSV with header `name\timage\tstate\thealth\trestarts`
    (`crates/domovoi-agent/src/requests/tools.rs`); fails with "Docker: ..." when Docker
    is off.
  - `ListeningPorts`: one listener per line, "no listeners outside loopback" when none.
  - `Journal { unit: None, since_minutes, lines, kernel }`: raw `journalctl` lines, max
    1440 minutes and 500 lines, **no priority filter**.
  - `RunReadSkill { skill, args }`: any installed **read** skill.
- The registry also keeps the agent's checks (`Registry::checks(host)`) and skills
  (`Registry::skills(host)`).
- Notes (`crates/domovoi-core/src/note.rs`, `NoteRepo` in
  `crates/domovoi-core/src/ports/repos/knowledge.rs`: `upsert`, `get`, `list(host)`,
  `search`) are Markdown per host, shown on the Notes page, and triage already reads
  them (`crates/domovoi-engine/src/triage/context.rs`, `notes()`, and the `notes_search`
  tool). Storing the profile as a generated note makes it visible, searchable and
  available to the model without a new table.
- `SettingsRepo` (`get`/`set` JSON by key) is a core port the hub's store implements;
  per-host keys look like `host.<name>.checks`.
- Hub files near the 500-line limit: `runtime/mod.rs` (432), `runtime/schedulers.rs`
  (427). Put new code in a new module and add only a few lines there.
- The hub's own machine has no agent unless one is installed or `--with-local-agent` is
  used (see prompt 02); it gets no profile until it does.

---

## 24a · Starter skills for the baseline: `log_summary` and `service_list`

### Read these files

- `examples/skills/system_overview/` (layout: `skill.toml`, executable `run`, `README.md`)
- `docs/book/src/guide/skills.md` (starter skills table)

### Steps

1. `examples/skills/log_summary/`: `risk = "read"`, `timeout = "30s"`, no params,
   `max_output_bytes = 32768`, description "Warnings and errors per program over the
   last 24 hours, and the 20 most recent errors." The `run` script (`#!/bin/sh`,
   `set -u`, mode 755) prints, when `journalctl` exists:
   - `== warnings and errors per program, last 24 h`:
     `journalctl --priority=warning --since=-24h --no-pager --quiet --output=short | awk '{ p = $5; sub(/\[.*/, "", p); sub(/:$/, "", p); n[p]++ } END { for (p in n) printf "%7d %s\n", n[p], p }' | sort -rn | head -30`
   - `== most recent errors`:
     `journalctl --priority=err --since=-24h --no-pager --quiet --output=short-iso --lines=20`
   and "journalctl not available" otherwise.
2. `examples/skills/service_list/`: same layout, description "Running and failed systemd
   services, and enabled timers.", `timeout = "15s"`, `max_output_bytes = 32768`. The
   script prints `systemctl list-units --type=service --state=running --no-legend --plain --no-pager`,
   then `--state=failed`, then `systemctl list-timers --no-legend --no-pager`, each
   under a `== title` line, and "systemctl not available" when it is missing.
3. `README.md` for each (two lines, like the others). Add both to the table in
   `docs/book/src/guide/skills.md` and make the count in that page match the number of
   folders in `examples/skills/` (prompt 11a may have added two more).
4. Run `cargo run -p domovoi -- check skills examples/skills` and fix what it reports.

### Done when

- `cargo test -p domovoi --test cli` passes; both skills are listed as valid.

---

## 24b · Collect, store and show the profile

### Steps

1. **Keep the facts.** Add `pub facts: HostFacts` to `Live` in `registry.rs`, fill it
   with `hello.facts` in `session.rs`, and add `Registry::facts(host) -> Option<HostFacts>`
   (from the live entry). Expose `AgentServer::host_facts(host)` and
   `AgentServer::host_checks(host)` in `agent_server/mod.rs`.
2. **The profile type.** New module `crates/domovoi-hub/src/profile/` (`mod.rs`,
   `collect.rs`, `render.rs`; declare it in `lib.rs`):
   ```rust
   /// What Domovoi knows about a machine at one moment.
   #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
   pub struct HostProfile {
       pub collected_at: Timestamp,
       pub agent_version: String,
       pub facts: ProfileFacts,              // hostname, os, kernel, arch, cpu_count, mem_total_bytes, boot_time (copied from HostFacts)
       pub containers: Vec<ContainerLine>,   // name, image, state, health, restarts (parsed from docker_ps TSV)
       pub listeners: Vec<String>,           // listening_ports lines
       pub skills: Vec<String>,              // "name (risk)"
       pub checks: Vec<String>,              // enabled check keys
       pub sections: Vec<ProfileSection>,    // title, text (raw, redacted, cut to 8 KiB), ok: bool
       pub note: Option<NoteId>,             // the generated note, reused on refresh
   }
   ```
   Saved with `SettingsRepo::set(store, &format!("host.{host_id}.profile"), ..)`.
3. **Collect** (`collect.rs`), `pub async fn collect(deps: &ProfileDeps, host: &Host) -> Result<HostProfile, ProfileError>`
   where `ProfileDeps` holds `server: AgentServer` (or the link and the facts lookup),
   `store: Arc<Store>`, `clock`, `redactor: Arc<dyn Redactor>`. Fail with
   `ProfileError::NotConnected` unless the host's presence is `Connected`. Then run, one
   after another, each with a 20 s deadline (`clock.now() + 20 s`), recording a failure
   as a section with `ok: false` and the error text instead of stopping:
   - `HostOverview` → section "Overview".
   - `DockerPs { all: true }` → parse the TSV (skip the header, split on `\t`, ignore
     short lines) into `containers`; section "Containers".
   - `ListeningPorts` → `listeners` (non-empty lines); section "Listening ports".
   - `Journal { unit: None, since_minutes: 1440, lines: 100, kernel: true }` → section
     "Kernel messages (24 h)".
   - For each of `log_summary`, `service_list`, `hardware_health`, `power_status` that is
     an installed **read** skill on the host (`link.skills(host)`):
     `RunReadSkill { skill, args: BTreeMap::new() }` → a section named after the skill.
   - `facts`, `agent_version`, `skills` and enabled `checks` come from the registry.
   Run every text through `redactor.redact` before keeping it.
4. **Render** (`render.rs`), `pub fn markdown(host: &Host, p: &HostProfile) -> String`:
   - First line: "_Generated by Domovoi on {date}. It is replaced when the profile
     refreshes; write your own notes separately._"
   - `## Summary`, at most ~2 KiB: OS and kernel, arch, CPUs, RAM (GiB), the
     memory/swap/mount lines from the overview, "N containers (M running): name (image),
     …" (first 30), listening ports (first 20), "skills: …", "checks: …", and the top 5
     lines of the `log_summary` counts when present.
   - `## Details`: each section as `### {title}` followed by its text in a fenced code
     block.
   Cut the whole note to 64 KiB.
5. **Save** (`mod.rs`, `pub async fn refresh(deps, host) -> Result<HostProfile, ProfileError>`):
   collect, render, `NoteRepo::upsert` a `Note { id: previous note id or NoteId::new(), host: Some(host.id), title: format!("Host profile: {}", host.name), body_markdown, tags: vec!["host-profile".into(), "generated".into()], updated_at: now }`,
   then save the JSON. Log `info` "host profile collected" with host and section count.
6. **When it runs** (`mod.rs`, `pub fn spawn(deps, shutdown) -> JoinHandle<()>`): every
   minute (a `clock.sleep_until` loop with the shutdown token; `every()` in
   `runtime/every.rs` is `pub(super)`, make it `pub(crate)` if you want to reuse it),
   look at every host with
   a connected agent and refresh **one** host per tick when any is true:
   - it has no stored profile (this is the "first start" case);
   - the stored `facts` differ in `os`, `kernel`, `mem_total_bytes` or `cpu_count`, or
     `agent_version` differs;
   - the stored profile is older than 7 days.
   Spawn it from `runtime/mod.rs` next to the other runtime tasks (a few lines).
7. **API** (new `crates/domovoi-hub/src/api/profile.rs`):
   - `GET /api/v1/hosts/{name}/profile` → `ProfileDto { collected_at, markdown, note_id, sections: Vec<{ title, ok }> }`,
     404 when none yet.
   - `POST /api/v1/hosts/{name}/profile` → refresh now and return the DTO; 409 "the
     agent is not connected". Add it to the `writes` list in
     `crates/domovoi-hub/tests/gates.rs`.
   The API needs the profile deps: add `pub profiles: Option<crate::profile::ProfileDeps>`
   (or a small handle type) to `ApiDeps` and fill the three construction sites
   (`crates/domovoi-hub/src/runtime/mod.rs`, `crates/domovoi-hub/examples/dev_hub.rs`,
   `crates/domovoi-hub/tests/hub/api_harness.rs`). Regenerate OpenAPI.
8. **Web.** On the host page add a **Profile** tab
   (`web/src/features/hosts/ProfileTab.tsx`): the Markdown, "Collected {relative time}",
   failed sections listed as "Not available: {title}", a **Refresh** button, and a link
   to the note. With no profile: "Collecting the first profile…" when the agent is
   connected (poll every 10 s), otherwise "Connect an agent to profile this machine."
   Hint: "What this machine looks like when things are normal. Triage reads it to tell
   new problems from the usual state." On the Notes page, show a "Generated" tag on
   notes tagged `generated`.
9. **Docs.** A "Host profile" section in `docs/book/src/concepts/hub-and-agent.md`: what
   is gathered, when, where it is shown, that it stays on the hub, and that editing the
   note is pointless because refreshes replace it.

### Tests

- `collect`: with `domovoi_testkit::FakeAgentLink` answering each tool, the profile has
  parsed containers and listeners, the four skills only when installed and read, and a
  failing tool becomes an `ok: false` section without failing the whole profile.
- `render`: the summary contains OS, RAM and container names; the note is at most
  64 KiB; a secret known to the redactor never appears.
- The scheduler picks a connected host with no profile, skips a disconnected one, and
  re-profiles when the kernel changes (fake clock and fake registry data).
- API: GET 404 before, POST then GET returns the markdown; POST for a disconnected host
  is 409.
- Web: the tab renders the profile and Refresh posts.

---

## 24c · Use the profile in triage and chat, and track changes

### Steps

1. **Triage context.** In `crates/domovoi-engine/src/triage/context.rs`, add a part
   "Host profile (the normal state of this host)" right after "Incident": from
   `deps.notes.list(Some(incident.host()))` take the newest note tagged `host-profile`,
   keep the text before `"\n## Details"`, and let `build()` cut it to `PART_BYTES` as it
   does for the other parts. In `notes()`, leave out notes tagged `host-profile` so the
   profile is not sent twice. Tell the model in
   `crates/domovoi-engine/prompts/triage_system.md` (under "What you can do"): "The host
   profile describes the host when it was healthy; compare against it to tell new
   problems from the usual state."
2. **Changes.** New `crates/domovoi-hub/src/profile/diff.rs`:
   `pub fn changes(old: &HostProfile, new: &HostProfile) -> Vec<String>` with one line
   per difference: container added / removed / image changed, listener added / removed,
   skill added / removed, OS, kernel, RAM or CPU count changed. In `refresh()`, when an
   old profile exists, compute the changes, prepend them (dated) to a list of the last
   20 changes kept in `HostProfile` (`pub changes: Vec<String>`, `#[serde(default)]`),
   and render them as `## Recent changes` between Summary and Details, so the model sees
   them inside the part from step 1.
3. **Chat.** Incident follow-ups (prompt 21) use `context::build`, so they get the
   profile automatically. For the general chat (prompt 22b), when a host is chosen,
   append that host's profile summary (the same text before `## Details`) to the
   overview the API builds.

### Tests

- Engine: with a `host-profile` note for the incident's host, the first triage request
  contains its summary but not its Details, and the note is not listed again under
  "Host notes".
- `diff::changes`: one test per kind of change; identical profiles give no lines.

### Done when (all of 24)

- A new agent's machine is profiled within a minute of connecting, visible on the host
  page and as a generated note, refreshed weekly and when the machine changes, and the
  model sees the summary and recent changes during triage and chat.
- All tests pass; no file over 500 lines; OpenAPI regenerated.
