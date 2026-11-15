# 25 — Capability registry

| | |
|---|---|
| Branch | `capability-registry` |
| Milestone | `M2` |
| Depends on (must be merged) | `23, 24` |
| Parallel with | `39 (order-flexible; never concurrent)` |
| Produces (shared contracts) | `Availability` + registry/availability/target-resolution services + read-only inventory API/UI |
| Consumes | `24`'s types, `20`'s 1.2 catalogue wire |
| Launch gate | `none beyond the dependencies` |
| Suggested model | `large` |

## 1. Objective

Replace the narrow per-host skill wrapper (`PolicyLink`, `crates/domovoi-hub/src/agent_server/skill_policy.rs`)
with one hub-owned **capability registry** that answers, per `(host, contract)`, *is this capability
available here, and if not, why not*.

The registry is the single place where five kinds of fact are combined:

1. **Contracts** — `domovoi_core::capability::ContractIndex` from prompt 24. The hub's compiled set is
   the only source of hub-known contract ids. Nothing an agent sends can add one.
2. **The agent catalogue** — what this host's agent actually implements: `hello.skills[]` for the
   `skill.<name>` family and `get_catalog` pages for everything else (wire 1.2, prompt 20).
3. **Host state** — `HostAvailability` (connected, never seen, gone, incompatible) plus the 1.2 status
   fields (`skill_runner_status`, `helper_status`, `runs_as_root`, `ceiling_summary`).
4. **Hub enablement** — the existing `host.<name>.skills` setting, unchanged, now read by the registry
   instead of by a link wrapper, and the new raise-only skill floor `host.<name>.floors`.
5. **Target identity** — resolving a target *name* (`plex`) to the canonical identity a plan binds
   (`container:{host}/{full container id}`), per RW-34.

After this prompt the hub can answer `availability(host, contract)` for every contract in the index,
enforce hub enablement at the one execution boundary and the one read boundary rather than in a wrapper,
and show a read-only inventory at `/capabilities`. It evaluates **no policy**: no permit, no approval, no
`Contain`, no effect class decision. "Available" means "the hub would offer this here"; it never means
"approved".

## 2. Architectural context

**RW-07 — hub-owned contracts (owner decision D-02).** Agents advertise implementation facts only.
Which contract exists, and how dangerous it is, is decided in `domovoi-core` (prompt 24). This prompt adds
the other half of RW-07: *which of those contracts a given host can execute right now*. That is a hub
fact, computed from the agent's own report plus hub state, never accepted ready-made from the agent.

**RW-34 — stable target identity.** Policy rules name targets by name; permits and plan hashes bind a
canonical identity. This prompt owns the resolver that maps one to the other, the container-name grammar
check that must happen *before* resolution (`[a-zA-Z0-9][a-zA-Z0-9_.-]+`), and the rule that a rename
keeps identity while a recreate (new container id) changes it.

**RW-39 — three identities (D-24, D-29).** The control agent, the isolated skill runner and the root
helper do not exist as separate identities yet (`H1`, prompt 47). Their 1.2 status fields are reported
honestly (`absent`), and that must not be turned into a claim that skills or privileged operations are
broken. Section 6.8 fixes exactly what each status field does to availability.

**04 §8.3 / D-18 — the host-local ceiling.** `agent.toml [unattended]` bounds *policy-authorized*
(unattended) execution: exact contract id, explicit targets, rate, declared constrainable arguments,
`max_ttl`, `max_active`. The hub cannot widen it and does not own it. The registry consumes the agent's
**summary** of it (`hello.ceiling_summary`, wire 1.2) and reports whether it covers the contract; it does
not parse `agent.toml` and cannot change the ceiling.

**04 §11.1 — the availability list.** Availability per `(host, contract)` is
`Available | NotInstalled | DisabledByHub | NotAllowedOnHost (allow_risk/ceiling) | Unavailable(reason) |
HostUnknown | HostAbsent | HostIncompatible(protocol)`, computed from contracts × agent catalogue (by
hash, paged) × hub enable state × ceiling summary. Section 6.1 fixes the eight variants and their order.

**04 §7 / RW-10 — the availability-versus-authorization distinction.** These are different questions with
different owners:

| Question | Owner | Answer | Can it refuse a run? |
|---|---|---|---|
| Is `container.restart` available on `media`? | this prompt | `Availability` | only for the states section 6.1 names |
| May this specific request run unattended? | prompt 39 (`evaluate`), prompt 22 (`Authorization`, `PolicyPermit`) | `Allow`/`RequireApproval`/`Deny` | yes |
| Will the agent accept it? | the agent, locally | its own refusal | yes |

Consequences that this document must be read with:

- **The registry never evaluates policy.** It does not read a rule, does not construct `Authorization`,
  `PolicyPermit` or `HumanApproval`, and cannot approve anything (`HumanApproval::new` stays confined to
  `crates/domovoi-hub/src/auth/`).
- **Available ≠ approved.** `Available` on a `Destructive` contract says only "this host implements it".
  Approval is still required by prompt 39's default and D-06.
- **Unavailable ≠ forbidden.** A `NotAllowedOnHost` or `Unavailable(..)` answer explains the hub's view for
  the inventory. It is not an authorization decision, and section 6.1 says precisely which states the
  boundaries refuse on.
- **D-23.** `Deny` (not offered for approval) is a policy outcome and lives in prompt 39. Nothing here
  produces it.

## 3. Repository context

All paths are relative to the repository root. Every line reference below was **verified in this
checkout** on branch `phase7-inventory`; re-open each one before editing and report any difference.

### 3.1 What exists today

| Fact | Where (verified) |
|---|---|
| `AgentLink` has six items and **no** catalogue method: `run_tool`, `run_skill`, `run_check`, `presence`, `availability`, `skills` | `crates/domovoi-core/src/ports/agent_link.rs:137-182` |
| `HostAvailability { Known { since }, Unknown, Absent { since }, Incompatible { reported } }` | `crates/domovoi-core/src/ports/agent_link.rs:91-110` |
| `availability` has a default implementation derived from `presence` | `crates/domovoi-core/src/ports/agent_link.rs:170-178` |
| `LinkError { NotConnected, Incompatible, Timeout, Rejected(String), Transport(String) }` | `crates/domovoi-core/src/ports/agent_link.rs:113-131` |
| `SettingsRepo { get, set, set_many, list_prefix }` | `crates/domovoi-core/src/ports/repos/accounts.rs:13-26` |
| `HostRepo::list_hosts`, `HostRepo::live_agent(host) -> Option<AgentIdentity>` | `crates/domovoi-core/src/ports/repos/hosts.rs:35`, `:49` |
| `SkillPolicy`: key `host.<name>.skills`, value `{"disabled": [..]}`, methods `load`, `is_disabled`, `set`, `disabled_names`, `remember`; `updates: tokio::sync::Mutex<()>` held across the store write | `crates/domovoi-hub/src/agent_server/skill_policy.rs:22-141` |
| `PolicyLink` narrows `run_tool` (only `ReadToolCall::RunReadSkill`), `run_skill` and `skills`; forwards the rest | `crates/domovoi-hub/src/agent_server/skill_policy.rs:143-213` |
| `DISABLED_REASON = "the skill is disabled on this host"` | `crates/domovoi-hub/src/agent_server/skill_policy.rs:23` |
| Seven tests inside that module | `crates/domovoi-hub/src/agent_server/skill_policy.rs:286-449` |
| Live session state: `Live { session, since, version, facts, skills, checks, docker, outbox, pending, cancel }` | `crates/domovoi-hub/src/agent_server/registry.rs:31-43` |
| `Registry::skills(host)`, `facts`, `docker`, `session_state`, `presence`, `checks` | `crates/domovoi-hub/src/agent_server/registry.rs:245-288` |
| `hello.skills` lands in `Live.skills` when the session registers | `crates/domovoi-hub/src/agent_server/session.rs:31-43` |
| `Hello` today: `agent_id, protocol, agent_version, facts, capabilities, skills, checks, docker` | `crates/domovoi-proto/src/messages/handshake.rs:152-175` |
| `DockerReport` reports the engine/endpoint and availability, **no containers** | `crates/domovoi-proto/src/messages/handshake.rs:83-104` |
| `WsAgentLink` is an `AgentLink` over the registry and converts proto → core types | `crates/domovoi-hub/src/agent_server/link.rs:173-` |
| `PolicyLink` is constructed once in the runtime and exported from `agent_server` | `crates/domovoi-hub/src/runtime/mod.rs:220-223`, `crates/domovoi-hub/src/agent_server/mod.rs:46` |
| The skills API lists every announced skill and marks each `enabled` from `SkillPolicy` | `crates/domovoi-hub/src/api/hosts.rs:246-298`, specifically `:294` |
| The switch endpoint validates the name against the agent's catalogue, calls `SkillPolicy::set`, audits | `crates/domovoi-hub/src/api/settings_write.rs:430-486` |
| `ApiDeps.skill_policy: Arc<SkillPolicy>` | `crates/domovoi-hub/src/api/mod.rs:161-162` |
| Engine reads the skill list through the link at four places | `crates/domovoi-engine/src/actions/executor.rs:391`, `crates/domovoi-engine/src/triage/run.rs:71`, `:107`, `crates/domovoi-engine/src/triage/followup.rs:174`, `crates/domovoi-engine/src/triage/chat.rs:170` |
| `Evidence::new(link)` / `collect_evidence` (prompt 23's read boundary) | `crates/domovoi-engine/src/evidence/mod.rs` |
| `Registry` for the web app: `NAV_GROUPS` with a `Fleet` group containing Hosts, Skills, Logs | `web/src/app/nav.ts:62-75` |
| `PAGES.skills` lazy page | `web/src/app/pages.ts:40` |
| `/skills` route | `web/src/app/routes.tsx:75` |
| `useSkills` / `useSetSkillEnabled` and `keys.skills(name)` | `web/src/api/hooks/hosts.ts:51-59`, `:89-102`; `web/src/api/queryClient.ts:21` |
| Status vocabulary fallback: an unknown value renders as its raw text with tone `neutral` | `web/src/design/status.ts:83-93` |
| `lookup`-backed tables (`HOST_CONNECTIONS`, `CHECK_STATES`) and the one `tone.ts` entry point | `web/src/design/fleetVocab.ts:15`, `:107`; `web/src/design/tone.ts:152-159` |
| Highest migration is `0010_llm_admission.sql` | `crates/domovoi-store/migrations/` |

### 3.2 What is missing (this prompt creates it)

| Missing | Consequence |
|---|---|
| Any `capability` module in the hub or `Availability` type anywhere | nothing can answer the per-`(host, contract)` question |
| Any `get_catalog` request on `AgentLink` | the registry must define the port method and let `WsAgentLink` convert to and from `domovoi_proto`'s `GetCatalog`/`CatalogPage` |
| Any consumer of `hello.catalog_hash`, `ceiling_summary`, `skill_runner_status`, `helper_status`, `runs_as_root` | the 1.2 fields exist on the wire and are ignored today |
| Any hub-side parser for `docker_ps` or `docker_inspect` output | container names and ids cannot be read today: see 3.3 |
| Any stored skill floor | only the disable switch exists |
| Any capability inventory endpoint or page | the UI has `/skills` only |

### 3.3 Container inventory: what exists, and what is new

Verified in this checkout:

- `docker_ps` on the agent prints a **tab-separated table with a header**
  `name<TAB>image<TAB>state<TAB>health<TAB>restarts`, one container per line, and **no container id**
  (`crates/domovoi-agent/src/requests/tools.rs:70-90`, header at `:72`).
- `docker_inspect` on the agent returns Docker's own inspect document pretty-printed as text
  (`crates/domovoi-agent/src/requests/tools.rs:98-102`). That document contains the container `Id`; the
  hub receives it as `ToolOutput.text` through `AgentLink::run_tool`.
- The checks crate maps that document into `ContainerInfo`, which has **no id field**
  (`crates/domovoi-checks/src/containers/mod.rs:93-114`; mapping at
  `crates/domovoi-checks/src/containers/docker.rs:111-151`).
- `DockerReport` (hello) carries engine, endpoint, availability, locked, enabled, problem — no containers
  (`crates/domovoi-proto/src/messages/handshake.rs:83-104`).
- The hub's container-runtime endpoint serves only `DockerReport` plus the saved choice
  (`crates/domovoi-hub/src/api/host_docker.rs:14-118`).

So **no hub-side parser exists for either output, and no container id ever reaches the hub today.** This
prompt adds the parser as new hub code (section 6.3); it invents no wire field and no agent change:

```rust
// crates/domovoi-hub/src/capability/inventory.rs (new)
/// One container as the hub can identify it. `id` is the full 64-character Docker id.
pub struct ContainerRef { pub name: TargetName, pub id: ContainerId }

/// Reads the container names out of a `docker_ps` `ToolOutput.text`.
pub fn parse_ps(text: &str) -> Result<Vec<TargetName>, InventoryError>;
/// Reads the full id out of a `docker_inspect` `ToolOutput.text`.
pub fn parse_inspect_id(text: &str) -> Result<ContainerId, InventoryError>;
```

`ContainerId` is a new core newtype over `String`, exactly 64 ASCII lowercase hex characters; a short id
(12 characters) is refused rather than expanded, because RW-34 binds the **full** id. `parse_ps` skips the
header line, splits on `\t`, takes column 0, and returns names in output order; a line with no `\t` is
counted and skipped (never fatal). `parse_inspect_id` reads the top-level `"Id"` string of the JSON
document and refuses anything else.

## 4. Exact scope

No new crate, no new protocol message, no migration, no new table.

### 4.1 `domovoi-core`

| File | Change |
|---|---|
| `crates/domovoi-core/src/capability/availability.rs` | **(new)** `Availability`, `NotAllowedOnHost`, `UnavailableReason`, `AvailabilityEntry`, `code()`, `describe()` |
| `crates/domovoi-core/src/capability/catalog.rs` | **(new)** `CatalogueState`, `CatalogueGap`, `CatalogSnapshot`, `CatalogImplementation`, `CatalogQuery`, `CatalogPageData` |
| `crates/domovoi-core/src/capability/target.rs` | **(new)** `ContainerId`, `TargetQuery`, `ResolvedTarget`, `TargetError`, `TargetUnresolved`, `validate_container_name` |
| `crates/domovoi-core/src/capability/mod.rs` | add the three modules and their re-exports; keep prompt 24's items untouched |
| `crates/domovoi-core/src/capability/tests.rs` | add the unit tests of section 12 (new file if 24's file is already near 500 lines; otherwise a sibling `tests_registry.rs`) |
| `crates/domovoi-core/src/ports/capability.rs` | **(new)** the `CapabilityRegistry` port (read-only, section 6.2) |
| `crates/domovoi-core/src/ports/target.rs` | **(new)** the `TargetResolver` port (section 6.3) |
| `crates/domovoi-core/src/ports/mod.rs` | register and re-export `capability::CapabilityRegistry` and `target::TargetResolver` |
| `crates/domovoi-core/src/lib.rs` | re-export `Availability` at the crate root (the registry's shared contract, per `prompts/README.md`) |

Nothing else in `domovoi-core` changes. In particular `ReadTool`, `ReadToolCall`, `Risk`,
`SkillSummary`, `AgentLink`'s existing methods and prompt 24's types stay exactly as merged.

### 4.2 `domovoi-hub`

| File | Change |
|---|---|
| `crates/domovoi-hub/src/capability/mod.rs` | **(new)** module doc and re-exports |
| `crates/domovoi-hub/src/capability/registry.rs` | **(new)** `HubRegistry` implementing the port; the only place that combines contracts, catalogue, host state, enablement, floors and ceiling |
| `crates/domovoi-hub/src/capability/catalogue.rs` | **(new)** the paged walk, the `(host, hash)` cache, the snapshot state machine |
| `crates/domovoi-hub/src/capability/enablement.rs` | **(new)** the moved `SkillPolicy` internals (`host.<name>.skills`), keeping `DISABLED_REASON` and `host_skills_key` |
| `crates/domovoi-hub/src/capability/floors.rs` | **(new)** `host.<name>.floors`, raise-only write, load and lookup |
| `crates/domovoi-hub/src/capability/ceiling.rs` | **(new)** wire ceiling summary → `CeilingSummary` |
| `crates/domovoi-hub/src/capability/inventory.rs` | **(new)** `parse_ps`, `parse_inspect_id`, `ContainerRef` |
| `crates/domovoi-hub/src/agent_server/skill_policy.rs` | **deleted**; its seven tests move to `capability/enablement.rs` unchanged apart from the type name |
| `crates/domovoi-hub/src/agent_server/mod.rs` | drop the `skill_policy` module and its re-exports |
| `crates/domovoi-hub/src/agent_server/link.rs` | add the `get_catalog` port method: build `Message::GetCatalog`, convert `CatalogPage` back to core types |
| `crates/domovoi-hub/src/api/capability.rs` | **(new)** `list_capabilities` handler and DTOs |
| `crates/domovoi-hub/src/api/mod.rs` | register the route and the schema; replace `ApiDeps.skill_policy` with `ApiDeps.capabilities: Arc<HubRegistry>` |
| `crates/domovoi-hub/src/api/hosts.rs` | the `enabled` flag comes from the registry; no DTO field changes |
| `crates/domovoi-hub/src/api/settings_write.rs` | `set_skill_enabled` calls the registry's enablement; behaviour, status codes and audit kind unchanged |
| `crates/domovoi-hub/src/runtime/mod.rs` | build `HubRegistry` instead of `SkillPolicy`/`PolicyLink`; pass the raw `server.link()` to the engine and the registry to the engine and the API |
| `crates/domovoi-hub/tests/hub/skills.rs` | keeps every existing test; the helper at `:67` loads the new type |
| `crates/domovoi-hub/tests/hub/api_harness.rs` | `:406-408` builds a registry instead of a `SkillPolicy` |
| `crates/domovoi-hub/tests/hub/availability.rs` | `:114-115` drops the `PolicyLink` wrapper |
| `crates/domovoi-hub/tests/hub/profile.rs` | `:283-287` drops the `PolicyLink` wrapper |
| `crates/domovoi-hub/tests/hub/capability.rs` | **(new)** the registry and endpoint tests |
| `crates/domovoi-hub/examples/dev_hub.rs` | `:504-505` builds a registry |
| `crates/domovoi-hub/tests/gates.rs` | extend with the boundary gate of section 12 |

### 4.3 `domovoi-engine`

| File | Change |
|---|---|
| `crates/domovoi-engine/src/evidence/mod.rs` | `Evidence::new(link, registry)`; enablement inside `collect_evidence`, replacing prompt 23's structural `Risk::Read` shortcut |
| `crates/domovoi-engine/src/actions/execute.rs` | the enablement check before `run_skill` (prompt 23's single execution path) |
| `crates/domovoi-engine/src/actions/executor.rs` | `:391` uses `capabilities.enabled_skills(host)` |
| `crates/domovoi-engine/src/triage/run.rs` | `:71`, `:107` use `capabilities.enabled_skills(host)` |
| `crates/domovoi-engine/src/triage/followup.rs` | `:174` likewise |
| `crates/domovoi-engine/src/triage/chat.rs` | `:170` likewise |
| `crates/domovoi-engine/src/lib.rs` or the deps structs | carry `Arc<dyn CapabilityRegistry>` into the action and triage dependencies |
| `crates/domovoi-engine/src/targets/mod.rs` | **(new)** `TargetService`, the `TargetResolver` port's implementation: grammar, `docker_ps`/`docker_inspect` through `collect_evidence`, host identity, unit refusal (section 6.3) |

### 4.4 `domovoi-agent` and `domovoi-testkit` — the catalogue the page walk reads

The registry can only be complete if some agent answers `get_catalog` and announces
`hello.catalog_hash`. Prompt 20 froze both on the 1.2 wire and assigned `catalog_hash` to this prompt
(`20:142`); an agent that never answers would leave every agent-executed contract
`Unavailable(CatalogueIncomplete)` for ever. This prompt therefore owns the agent's side of that contract.
It adds **no new message and no version change**: only the handler for the message 1.2 already defines.

| File | Change |
|---|---|
| `crates/domovoi-agent/src/requests/catalog.rs` | **(new)** `catalog_entries()` over the loaded skill catalogue and `ReadTool::ALL`, and `page(entries, query)` |
| `crates/domovoi-agent/src/requests/mod.rs` | dispatch `Message::GetCatalog` to the handler; answer `catalog_page`; an unknown page shape is `invalid` |
| `crates/domovoi-agent/src/link/handshake.rs` | build `hello.catalog_hash` from the same entry list, so the announced hash and the pages can never disagree |
| `crates/domovoi-core/src/capability/catalog.rs` | the pure `catalog_hash(&BTreeMap<ContractId, ContractVersion>) -> String` used by the agent (and by a hub test to check a page set) |
| `crates/domovoi-testkit/src/capability.rs` | **(new)** `FakeCapabilityRegistry` implementing `CapabilityRegistry` |
| `crates/domovoi-testkit/src/link.rs` | `FakeAgentLink` records `get_catalog` calls and serves a scripted catalogue |
| `crates/domovoi-testkit/src/lib.rs` | re-export the fake |
| `crates/domovoi-proto`, `crates/domovoi-store` | **no change** |

Rules for the agent side, all in section 6.4.1:

- The entry list is exactly: every `ReadTool` for which prompt 24's `contract_id_of` returns `Some`
  (the ten non-skill tools), plus `ContractId::skill(name)` version 1 for every skill the loaded
  catalogue reports in `summaries()`. Nothing else. A tool without a contract id is not listed.
- Entries are sorted by contract id and paged by index, so two calls with the same `(page, page_size)`
  always return the same slice and `next_page` is `None` on the last page.
- `catalog_hash` is `SHA-256` over the canonical text of that sorted list (section 6.4.1), lower-case
  hex. It is computed from the same list the handler serves, at the same moment, so a page walk that
  matches the announced hash is a walk of the list the agent will actually execute.
- The descriptor is `{}`. The hub ignores it (section 6.4), and filling it would imply the agent
  describes its own authority, which RW-07 rejects.
- No skill is added from a manifest that failed the catalogue load checks, and `catalog_hash` therefore
  changes when a skill appears, disappears or is replaced.

### 4.5 `web/`

| File | Change |
|---|---|
| `web/src/features/capabilities/CapabilitiesPage.tsx` | **(new)** |
| `web/src/features/capabilities/CapabilityList.tsx` | **(new)** |
| `web/src/features/capabilities/capabilityText.ts` | **(new)** owner-facing sentences |
| `web/src/features/capabilities/capabilities.module.css` | **(new)** tokens only |
| `web/src/features/capabilities/capabilities.test.tsx` | **(new)** section 12 |
| `web/src/api/hooks/capabilities.ts` | **(new)** `useCapabilities(name)` |
| `web/src/api/queryClient.ts` | add `capabilities: (name: string) => ["hosts", "capabilities", name]` |
| `web/src/api/types.ts` | `export type Capability = Schemas["CapabilityDto"];` and the envelope type |
| `web/src/api/openapi.json`, `web/src/api/schema.d.ts` | **regenerated**, never hand-edited |
| `web/src/design/fleetVocab.ts` | add `CAPABILITY_STATES` next to `CHECK_STATES` (`:107`) |
| `web/src/design/status.ts` | add `CapabilityState` to the type union block (`:55-77`) |
| `web/src/design/tone.ts` | add `capabilityStateMeta` and one `VOCABULARIES` row (`:188-208`) |
| `web/src/design/icons.tsx` | add `capability: Boxes` to the icon map (`:54-105`) |
| `web/src/app/nav.ts` | one item in the `Fleet` group (`:62-75`) |
| `web/src/app/pages.ts` | `capabilities: page(() => import("../features/capabilities/CapabilitiesPage").then((m) => m.CapabilitiesPage))` |
| `web/src/app/routes.tsx` | `<Route path="/capabilities" element={<PAGES.capabilities.Page />} />` |

`web/src/features/skills/**` keeps working unchanged: the `/skills` page and its switch stay exactly as
they are, and its `enabled` flag keeps coming from the same setting.

### 4.6 `docs/` and `ARCHITECTURE.md`

Section 14 lists them. `ARCHITECTURE.md` is edited here because decision D-15 makes the last M2 prompt
update it once.

## 5. Explicit non-goals

- No policy: no rule type, no `evaluate`, no `Decision`, no `Deny`/`RequireApproval`/`Allow`, no
  explanation or simulator (prompt 39, 40).
- No authorization: no `Authorization`, `PolicyPermit`, `AuthorizationKind`, `expected_safety`, permit
  issuance, approval, step-up or typed confirmation (prompts 22, 39, 45).
- No execution: no new call to `AgentLink::run_skill` or `run_tool`, no new dispatch transition, no
  second execution path (prompt 23 owns both boundaries). The registry performs exactly one kind of
  agent call: `get_catalog` pages, and the read tools `docker_ps`/`docker_inspect` **through the existing
  `collect_evidence` boundary** for target resolution.
- No new capability contracts. `container.restart`, `unit.restart` and `firewall.*` belong to prompts 45
  and 46; do not add placeholders. `TargetKind::NetworkSource` does not exist in prompt 24 and is not
  added here; network targets arrive with 46.
- No installer, no packages, no capability center, no key enrolment, no helper or skill-runner setup, no
  M8/M9 controls (prompts 41, 44, 47, 49).
- No editing from the UI: the M2 inventory is read-only. The `/skills` switch is the only switch that
  exists, it is not moved, and it is not generalised.
- No new wire message, no protocol version change, no `agent.toml` parsing, no ceiling editing. The agent
  change is confined to answering the frozen `get_catalog` request and filling the frozen
  `hello.catalog_hash` from the same list (section 6.4.1); it adds no message, field or version.
- No new table, no migration, no column. The skill floor is one settings key.
- No unit identity claim: R-8 is open. No unit mutation is authorized and no test may assert an
  empirically accepted unit identity (section 6.3, section 12).
- No container-runtime write path: D-24 stands. Resolution reads only.
- No second host identity model. Host identity is `HostId` plus the enrolled agent key fingerprint; no
  `HostName`-based identity, no new host id type.

## 6. Required interfaces and contracts

Mark: **fixed** = names, variants and semantics must match; **convention** = follow repository style.

### 6.1 `Availability` (core, **new**, owned by this prompt) **fixed**

```rust
// crates/domovoi-core/src/capability/availability.rs
/// Whether the hub can offer one contract on one host, and why not.
///
/// Availability is a report, not an authorization decision: `Available` never means approved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Availability {
    /// The host implements it, the hub enables it, the host's own rules allow it, and nothing is
    /// missing at the resolution level.
    Available,
    /// The host's agent does not implement the contract. Only ever produced from a **complete**
    /// catalogue.
    NotInstalled,
    /// The owner switched it off in the hub (`host.<name>.skills`). The agent still has it.
    DisabledByHub,
    /// The host's own configuration refuses this class: no unattended ceiling entry, or the agent's
    /// `[policy] allow_risk` does not cover it.
    NotAllowedOnHost(NotAllowedOnHost),
    /// Something the host needs for this contract is missing or broken.
    Unavailable(UnavailableReason),
    /// The host has had no agent session in this hub run.
    HostUnknown,
    /// The host had a session in this hub run and it ended.
    HostAbsent { since: Timestamp },
    /// The hub refused this host's agent: it speaks another protocol version.
    HostIncompatible { reported: String },
}

/// Why the host's own configuration refuses a contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotAllowedOnHost {
    /// No ceiling summary was reported, or the reported ceiling is disabled. Applies to non-`Read`
    /// classes only.
    CeilingAbsent,
    /// A ceiling exists but has no entry for this exact contract id (D-18: exact ids only).
    CeilingEntryMissing,
    /// The agent's local risk allowlist does not cover the contract's effect class. **Never produced
    /// in this milestone:** no wire field carries `[policy] allow_risk` (section 6.8).
    AllowRisk,
}

/// Why a host cannot attempt a contract it does implement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnavailableReason {
    /// The agent is unprivileged and reports no skill runner: skills have nowhere to run.
    SkillRunnerMissing,
    /// The agent reported the skill runner as unsupported.
    SkillRunnerUnsupported,
    /// The agent reported the skill runner as failing.
    SkillRunnerError,
    /// A helper-path contract on a host whose agent reports no helper.
    HelperMissing,
    /// The agent reported the helper as failing.
    HelperError,
    /// The catalogue walk did not finish: a page failed, the host disconnected, or the walk was cut.
    CatalogueIncomplete,
    /// The agent announced a catalogue hash that the pages do not match.
    CatalogueHashMismatch,
    /// The catalogue belongs to a hash the agent no longer announces.
    CatalogueStale,
    /// A container-targeted contract on a host whose agent reports no usable container runtime.
    ContainerRuntimeUnavailable,
}

impl Availability {
    /// Every variant, for the UI vocabulary test and the API DTO test.
    pub const ALL: [Availability; 8];
    /// The stable wire/API code: `available`, `not_installed`, `disabled_by_hub`,
    /// `not_allowed_on_host`, `unavailable`, `host_unknown`, `host_absent`, `host_incompatible`.
    pub fn code(&self) -> &'static str;
    /// The reason code, when there is one: `ceiling_absent`, `skill_runner_missing`, ...
    pub fn reason_code(&self) -> Option<&'static str>;
    /// A short owner-facing sentence. Never a raw code, never a secret.
    pub fn describe(&self) -> String;
    /// True only for `Available`.
    pub fn is_available(&self) -> bool;
}
```

**Semantics, one line each.**

| Variant | Means | Produced from |
|---|---|---|
| `Available` | The hub would offer it here | everything below says nothing is missing |
| `NotInstalled` | The agent does not implement it | a **complete** catalogue missing the id, or `hello.skills` missing a `skill.<name>` |
| `DisabledByHub` | the owner switched it off in the hub | `host.<name>.skills` containing the skill, for `skill.*` only |
| `NotAllowedOnHost` | the host's own rules refuse it | `ceiling_summary` (non-`Read` only); `AllowRisk` is defined but not produced in M2 |
| `Unavailable` | it is here but cannot run now | runner/helper status, catalogue state, container runtime |
| `HostUnknown` | never connected in this hub run | `HostAvailability::Unknown` |
| `HostAbsent` | connected before, gone now | `HostAvailability::Absent` |
| `HostIncompatible` | wrong protocol | `HostAvailability::Incompatible` |

**Precedence table** — evaluate top to bottom, first match wins. This order is fixed; a later prompt may
add inputs but may not reorder the rows.

| # | Condition | Result |
|---|---|---|
| 1 | `link.availability(host)` is `Unknown` | `HostUnknown` |
| 2 | `link.availability(host)` is `Absent { since }` | `HostAbsent { since }` |
| 3 | `link.availability(host)` is `Incompatible { reported }` | `HostIncompatible { reported }` |
| 4 | `contract.id.is_skill()` and `enablement.is_disabled(host, skill)` | `DisabledByHub` |
| 5 | the catalogue state for `host` is not `Complete` and the contract is agent-executed (see 6.4) | `Unavailable(CatalogueIncomplete)` or `Unavailable(CatalogueHashMismatch)` or `Unavailable(CatalogueStale)`, for the gap |
| 6 | the contract is not in this host's implementation list (complete catalogue, or `hello.skills` for skills) | `NotInstalled` |
| 7 | `contract.effect != EffectClass::Read` and the ceiling is absent, disabled or has no entry for the contract | `NotAllowedOnHost(CeilingAbsent)` / `NotAllowedOnHost(CeilingEntryMissing)` |
| 8 | a runner/helper/container-runtime input of section 6.8 refuses it | `Unavailable(reason)` |
| 9 | otherwise | `Available` |

Note on rows 5 and 6: row 5 precedes row 6 deliberately. An incomplete catalogue can never prove
`NotInstalled`, and a partial catalogue is never `Available`.

**Which states the boundaries refuse on.** Only these, and only here:

| State | `collect_evidence` (engine, prompt 23) | `execute` → `run_skill` (engine, prompt 23) | Inventory |
|---|---|---|---|
| `HostUnknown` | `EvidenceError::NotConnected` before any link call | refuses, as the executor does today | shown |
| `HostAbsent` | `EvidenceError::NotConnected` | refuses / waits, unchanged | shown |
| `HostIncompatible` | `EvidenceError::Incompatible` | refuses, unchanged | shown |
| `DisabledByHub` | `LinkError::Rejected(DISABLED_REASON)` — same text as today | same | shown |
| `NotInstalled`, `NotAllowedOnHost`, `Unavailable`, `Available` | **not** refused here | **not** refused here | shown |

`NotInstalled`, `NotAllowedOnHost` and `Unavailable` are deliberately not refusals at the boundary: the
agent applies its own `allow_risk`, manifest and plan-hash checks and produces its own error text, and a
human-approved action may legitimately run where unattended execution is refused (04 §7.3 hard limits turn
a ceiling gap into `RequireApproval`, not `Deny`). Refusing here would create a second authorization
decision inside the registry, which RW-10 forbids. Section 6.8 records the one case where this matters for
skills.

**Entry type**, used by the API and the UI:

```rust
/// One row of the inventory: a contract as this host sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AvailabilityEntry {
    pub contract: ContractId,
    pub version: ContractVersion,
    pub title: String,
    pub category: ContractCategory,
    pub effect: EffectClass,
    pub target_kind: TargetKind,
    pub execution: ExecutionPath,
    pub state: Availability,
    pub catalogue: CatalogueState,
    /// `hello.runs_as_root`: a label, never a refusal (section 6.8).
    pub root_agent: bool,
    /// True when the implementation came from `hello.skills[]` rather than a catalogue page.
    pub from_hello_skills: bool,
}
```

### 6.2 `CapabilityRegistry` — the port and the service **fixed**

The engine must consume the registry without depending on `domovoi-hub` (§4 R1). So the read-only query
API is a **port** in `domovoi-core::ports`, implemented by the hub and by the testkit.

```rust
// crates/domovoi-core/src/ports/capability.rs  (new)
/// The read-only capability view the execution and read boundaries consult.
///
/// Implemented by the hub's registry. Never evaluates policy, never approves, never dispatches.
#[async_trait]
pub trait CapabilityRegistry: Send + Sync {
    /// Whether this contract is available on `host`, and why not. May walk the agent catalogue on a
    /// miss; never holds an internal lock across an `.await`.
    async fn availability(&self, host: HostId, contract: &ContractId) -> Availability;

    /// This host's catalogue snapshot, already walked as far as it can be.
    async fn catalog(&self, host: HostId) -> CatalogSnapshot;

    /// The hub's compiled contract set. The only source of hub-known ids.
    fn contracts(&self) -> &ContractIndex;

    /// The effective effect class of `skill` on `host` (declared class raised by the floor), or
    /// `None` when the host does not announce that skill.
    fn effective_effect(&self, host: HostId, skill: &SkillName) -> Option<EffectClass>;

    /// Whether the hub enables this contract on `host`. In-memory, so the hot path does not wait.
    fn enabled(&self, host: HostId, contract: &ContractId) -> bool;

    /// The skills the hub may offer on `host`: announced by the agent **and** enabled here.
    fn enabled_skills(&self, host: HostId) -> Vec<SkillSummary>;
}
```

`enabled_skills` is the one method beyond the five this brief names on the port; it exists because prompt 23 removes
`PolicyLink`, and the four engine call sites that read `link.skills(host)` today
(`actions/executor.rs:391`, `triage/run.rs:71`, `:107`, `triage/followup.rs:174`, `triage/chat.rs:170`)
must not start seeing skills the owner disabled. `AgentLink::skills` itself stays unfiltered: it is the
agent's truth, used by the inventory and by the `/skills` page.

**Target resolution is a second port, not a registry method.** `resolve_target` deliberately does not
appear above, because resolution has to read the host through prompt 23's `collect_evidence`, which
itself consults the registry for availability; putting it on `CapabilityRegistry` would make the read
boundary and the registry call each other and force a construction cycle. Section 6.3 defines the
separate `TargetResolver` port and names the engine service that implements it.

**Hub service.**

```rust
// crates/domovoi-hub/src/capability/registry.rs (new)
pub struct HubRegistry { /* contracts: Arc<ContractIndex>, link: Arc<dyn AgentLink>,
                           hosts: Arc<dyn HostRepo>, store: Arc<dyn SettingsRepo>,
                           clock: Arc<dyn Clock>, enablement: Arc<Enablement>,
                           floors: Arc<Floors>, catalogue: Arc<CatalogueCache> */ }

pub struct RegistryDeps {
    pub contracts: Arc<ContractIndex>,
    pub link: Arc<dyn AgentLink>,
    pub hosts: Arc<dyn HostRepo>,
    pub store: Arc<dyn SettingsRepo>,
    pub clock: Arc<dyn Clock>,
}

pub enum RegistryError { Index(IndexError), Store(StoreError) }

impl HubRegistry {
    /// Validates the index once (prompt 24's `ContractIndex::new`) and loads the settings-backed
    /// state for every host.
    pub async fn new(deps: RegistryDeps) -> Result<Self, RegistryError>;

    /// One host's whole inventory, already computed. Used by the API.
    pub async fn inventory(&self, host: HostId) -> Vec<AvailabilityEntry>;

    /// Contracts the agent reported that the hub does not know. Diagnostics only; never offered.
    pub fn unknown_contracts(&self, host: HostId) -> Vec<String>;

    /// The floor write path (section 6.5).
    pub async fn set_floor(&self, host: &Host, skill: SkillName, floor: Option<EffectClass>)
        -> Result<(), FloorWriteError>;
}
```

Lock rules, which the reviewer will check:

1. `std::sync::Mutex` guards are cloned out and dropped before any `.await`; `tokio::sync::Mutex` is used
   only for the two settings write paths (`Enablement::set`, `Floors::set`), each held across exactly one
   store write, exactly as `skill_policy.rs:99-108` does today.
2. No transport, handler or engine function receives a guard, a `Mutex`, or a borrowed map.
3. `availability` never calls `AgentLink` while holding a lock. It clones the inputs it needs, awaits the
   catalogue walk if one is required, then takes the lock once to store the snapshot.

### 6.3 `TargetResolver` and target identity **fixed**

The port, in `domovoi-core`, next to the read and registry ports:

```rust
// crates/domovoi-core/src/ports/target.rs (new)
/// Resolves a target name to the canonical identity a plan would bind (RW-34).
///
/// Read-only. Implemented in the engine beside `collect_evidence`, because the reads it needs must go
/// through that one boundary. Never writes, never approves, never reaches `run_tool` itself.
#[async_trait]
pub trait TargetResolver: Send + Sync {
    async fn resolve(&self, query: TargetQuery) -> Result<ResolvedTarget, TargetError>;
}
```

`crates/domovoi-engine/src/targets/mod.rs` **(new)** implements it as `TargetService`, holding
`Arc<Evidence>` (prompt 23) and `Arc<dyn HostRepo>`. The hub builds it after the registry and passes it
to the inventory API and to whatever later prompt needs a plan identity; nothing in the engine or the
hub holds a second resolver.
 
Why not a `CapabilityRegistry` method: `collect_evidence` must ask the registry whether the read is
available and enabled, so a registry method that calls `collect_evidence` would close a call cycle and
force a construction cycle between `HubRegistry` and `Evidence`. Two ports, built in the order
`Enablement → HubRegistry → Evidence(link, registry) → TargetService(evidence, hosts)`, have neither.

```rust
// crates/domovoi-core/src/capability/target.rs (new)
/// A Docker container's full id: exactly 64 ASCII lowercase hex characters. A short id is refused.
pub struct ContainerId(String);           // Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize

/// What a caller wants identified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetQuery {
    Host { host: HostId },
    Container { host: HostId, name: TargetName },
    Unit { host: HostId, name: TargetName },
}

/// The identity a plan binds, plus what the caller needs to display and explain it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTarget {
    pub identity: TargetIdentity,
    /// The name the owner typed or the agent reported. Display only.
    pub name: TargetName,
    pub kind: TargetKind,
    /// The enrolled agent key's fingerprint, lowercase hex, computed by the hub from the enrolled
    /// public key. Never sent by the agent. `None` when the host has no enrolled agent.
    pub host_key_fingerprint: Option<String>,
}

pub enum TargetError {
    /// The target does not resolve now. Never optimistic, never a guess.
    Unresolved { query: TargetQuery, reason: TargetUnresolved },
    /// More than one container has this name (should not happen; reported, not chosen).
    Ambiguous { query: TargetQuery, count: usize },
    /// The name is not valid for its kind.
    InvalidName { query: TargetQuery, reason: ValidationError },
    /// The host cannot answer: not connected, incompatible, or no container runtime.
    HostUnavailable { host: HostId, state: Availability },
}

pub enum TargetUnresolved {
    ContainerVanished,
    /// The inspect document carried no full id: identity is not provable, so nothing is bound.
    ContainerIdentityUnproven,
    /// **R-8 is open.** Always returned for units in this milestone (section 6.3.3).
    UnitIdentityUnproven,
}

/// RW-34's container-name grammar, anchored: `[a-zA-Z0-9][a-zA-Z0-9_.-]+`, full match.
pub fn validate_container_name(text: &str) -> Result<TargetName, ValidationError>;
```

#### 6.3.1 Host

Identity is `TargetIdentity` `host:{HostId}` — the existing `HostId`, not a new type. The enrolled agent
key fingerprint is returned alongside (`host_key_fingerprint`) so a plan-hash binding has both halves of
RW-34's row without a second host identity model. The fingerprint is SHA-256 over the enrolled public key's
32 raw bytes, lowercase hex, computed in the hub (`capability/targets.rs`), never accepted from an agent.
Re-enrolment therefore changes the fingerprint and invalidates a bound plan; that invalidation is applied
by prompt 22's plan hash and prompt 26's reconciliation, not here — this prompt only reports both values.

`TargetQuery::Host` cannot fail while the host exists in the store; a host whose agent is `HostUnknown` is
still resolvable (the identity does not depend on a session), and `HostUnavailable` is only returned for
container and unit queries.

#### 6.3.2 Container

1. `validate_container_name` runs **before** any resolution. It is anchored, so `plex`, `my-container`,
   `a.b_c` pass and `plex `, `-plex`, `plex/1`, an empty string, a 129-character name and any text with
   whitespace or `/` fail with `TargetError::InvalidName`. As written in RW-34 the grammar needs at least
   two characters, so a one-character name is refused; do not "fix" that here.
2. Names come from `inventory::parse_ps` over the `docker_ps` result; ids from
   `inventory::parse_inspect_id` over one `docker_inspect` result per candidate name.
3. Exactly one match → `identity = container:{host_id}/{full id}`, `name` keeps what the caller typed.
4. No match, or an inspect document with no valid full id → `Unresolved(ContainerVanished |
   ContainerIdentityUnproven)`. Never fall back to the name, never bind a short id.
5. More than one match → `Ambiguous`. Never pick one.
6. A rename keeps the id and therefore the identity; a recreate has a new id and therefore a new
   identity. This is the whole point of binding the id (`target_identity_change_refused`, section 12).
7. Both reads go through the prompt 23 read boundary (`collect_evidence` with
   `EvidenceScope::Host(host)`), never through `AgentLink::run_tool` directly.
8. No caching at M2: a resolution is one `docker_ps` plus one `docker_inspect` per candidate, on demand,
   and nothing is refreshed on a timer. (A cache is prompt 45's problem, with its own freshness rule.)

#### 6.3.3 Unit — deliberately unresolved

**R-8 is open** (`99-open-decisions.md` R-8: "Effective-configuration identity for systemd units (fragment
+ drop-ins) across supported systemd versions"). RW-34's unit row needs `config_hash` over `FragmentPath`
plus every `DropInPaths` file, and nothing in the checkout can produce that: there is no unit inventory,
no fragment reader and no drop-in enumerator.

Therefore, in this milestone:

- `TargetQuery::Unit` **always** returns `TargetError::Unresolved { reason: UnitIdentityUnproven }`.
- The shape is implemented, documented and tested (`unit_identity_is_never_claimed`, section 12) so that a
  later prompt has the type and the refusal, not a redesign.
- No unit mutation is authorized by anything in this prompt, and no test, doc comment, UI string or PR
  description may state or imply that unit identity is accepted, verified or empirically proven.
- `unit.restart` is not a contract in M2 (prompt 45), so nothing calls this path yet. The refusal is what
  makes that safe rather than convenient.

#### 6.3.4 Network targets

Out of scope. `TargetKind` in prompt 24 has no `NetworkSource` variant, `TargetIdentity`'s `ipnet` scheme is
a validated string with no resolver, and `firewall.block_source` arrives with prompt 46. Do not add a target
kind, a CIDR parser or a resolver branch here.

### 6.4 Catalogue pagination consumption **fixed**

The port method (added to `AgentLink`, with a default so existing implementations keep compiling):

```rust
// crates/domovoi-core/src/ports/agent_link.rs — added method
/// Asks the agent for one page of its capability catalogue (wire 1.2).
///
/// Links that cannot page a catalogue return `Err(LinkError::Rejected(..))`, which the registry
/// treats as an incomplete catalogue. An agent too old to answer produces the same result through
/// `unsupported`.
async fn get_catalog(
    &self,
    host: HostId,
    query: CatalogQuery,
    deadline: Timestamp,
) -> Result<CatalogPageData, LinkError> {
    let _ = (host, query, deadline);
    Err(LinkError::Rejected("this link cannot page a capability catalogue".to_owned()))
}
```

```rust
// crates/domovoi-core/src/capability/catalog.rs (new)
pub struct CatalogQuery { pub page: u32, pub page_size: u32 }
pub struct CatalogPageData { pub entries: Vec<CatalogImplementation>, pub next_page: Option<u32> }
pub struct CatalogImplementation { pub contract: ContractId, pub version: ContractVersion }
```

`crates/domovoi-hub/src/agent_server/link.rs` implements it by building
`Message::GetCatalog(GetCatalog { page, page_size })` and converting `CatalogPage` (prompt 20's proto
types) into `CatalogPageData`. Conversion rules:

- `page_size` is clamped to `1..=100` before the request (`0` becomes `100`); prompt 20 already clamps a
  larger value, this just avoids sending nonsense.
- A `CatalogEntry.contract` string that is not a valid `ContractId` (prompt 24's grammar) fails the page:
  the whole snapshot is `Partial(page_malformed)` — a malformed entry is never skipped, because skipping
  would let a peer hide an implementation.
- `CatalogEntry.version` of `0` is `ContractVersion::new(0)` failing: same treatment.
- `CatalogEntry.descriptor` is **ignored**. It is opaque at the wire layer, and deriving a contract from
  it would let an agent define its own authority, which RW-07 rejects. M2 uses ids and versions only.

The walk (`capability/catalogue.rs`), all of it in one function with the state machine below:

| Rule | Value |
|---|---|
| Page size | 100 entries per request |
| Page bound | at most 50 pages per walk; a `next_page` beyond that ends the walk as `Partial(too_many_pages)` |
| Cursor | `next_page` from the previous page. It must strictly increase; a repeat or a step backwards ends the walk as `Partial(cursor_not_advancing)` |
| Per-page consistency | every page is converted and appended atomically; a failure leaves the previous snapshot intact and stores `Partial` instead |
| Deadline | one deadline per **walk** (30 s), not per page; a page that lands after it ends the walk as `Partial(deadline)` |
| Announced hash | `hello.catalog_hash`, 64 lowercase hex. Absent, empty or malformed → `Partial(hash_unannounced)` |
| Hash verification | the walk has no per-page hash, so `Complete` requires all four: every page arrived, `next_page` ended at `None`, the same session served the whole walk, and the announced hash is still the one announced when the walk started |
| Duplicate detection | a contract id appearing twice in one walk is `Partial(duplicate_entry)`; it is not de-duplicated silently |
| Stale detection | a cached snapshot whose hash differs from the hash currently announced is `Stale`; `catalog()` re-walks rather than serving it |
| Disconnect | a `LinkError::NotConnected`/`Transport`/`Timeout` mid-walk ends it as `Partial(disconnected)`; the host's availability then outranks the catalogue anyway (rows 1-3) |
| `unsupported` reply | `Partial(unsupported)` — never `Complete`, never `NotInstalled` |
| Snapshot key | the pair `(host, catalogue_hash)`: stored per host, replaced only when the announced hash changes, and marked `Stale` when they disagree |

```rust
pub enum CatalogueState {
    /// Every page arrived, the cursor ended, the session stayed up and the hash still matches.
    Complete { hash: String },
    Partial(CatalogueGap),
    Stale { announced: String, snapshot: String },
}

pub enum CatalogueGap {
    NotWalked, HashUnannounced, HashMalformed, Unsupported, PageFailed { page: u32 },
    PageMalformed { page: u32 }, CursorNotAdvancing { page: u32 }, TooManyPages,
    DuplicateEntry { contract: ContractId }, Disconnected { after_page: u32 }, Deadline,
}

pub struct CatalogSnapshot {
    pub host: HostId,
    pub announced_hash: Option<String>,
    pub state: CatalogueState,
    pub entries: BTreeMap<ContractId, ContractVersion>,
    pub unknown: Vec<String>,        // ids the hub does not know: diagnostics, never offered
    pub pages: u32,
    /// The session that was walked. A different session means the snapshot is not reusable.
    pub session_since: Option<Timestamp>,
    pub at: Timestamp,
}
```

**How `hello.skills` reconciles with `get_catalog`.** They answer different halves of one question and
must not be merged into one list:

| Source | Authoritative for | Fields it supplies |
|---|---|---|
| `hello.skills[]` | the `skill.<name>` family: which skills exist and how to build their contract | `SkillSummary` (name, description, `risk`, params, timeout, `manifest_hash`, `entry_script_digest`) |
| `get_catalog` pages | non-skill implementations plus descriptors: which non-skill contract ids the agent implements | id + version (descriptor ignored) |

Rules:

1. A `skill.<name>` contract is `NotInstalled` when `hello.skills` has no such name, **unless** a complete
   catalogue names it; the catalogue wins only for ids outside the `skill.` family, because the hub builds
   a skill's contract from `SkillSummary` and cannot build one from an id alone.
2. A catalogue page that names a `skill.<name>` id is recorded as an unknown/diagnostic entry, not as an
   implementation: it carries no summary, so the hub cannot describe the skill and must not offer it.
3. A non-skill contract id that the hub's `ContractIndex` does not contain goes into `unknown` and into the
   diagnostic list. It never creates a contract, never appears as a row, and never becomes `Available`.
4. Section 6.1 row 5 (catalogue incomplete) applies only to **agent-executed** contracts. Hub-executed
   contracts (`ExecutionPath::Hub`: `incident.history`, `notes.search`) are answered from the hub's own
   store and are never gated on the catalogue; skill contracts are gated on `hello.skills`, not on pages.

#### 6.4.1 The agent side of the catalogue

The page walk in section 6.4 has no meaning unless an agent answers it, so this prompt implements the
agent's half. It is not a new message: `get_catalog`, `catalog_page`, `CatalogEntry` and
`hello.catalog_hash` are 1.2's, frozen by prompt 20 (§8.3 and the `hello` table).

```rust
// crates/domovoi-agent/src/requests/catalog.rs (new)
/// Every implementation this agent offers: the ten contract-bearing read tools, then every skill
/// the loaded catalogue reports. Sorted by id; version 1 for skills.
pub fn catalog_entries(catalog: &SkillCatalog) -> BTreeMap<ContractId, ContractVersion>;

/// One page of `entries`, or `None` when `page` is past the end.
///
/// `page_size` is clamped to `1..=100`; the wire cap is prompt 20's.
pub fn page(entries: &BTreeMap<ContractId, ContractVersion>, query: &GetCatalog)
    -> CatalogPage;
```

1. **Which ids.** `ReadTool::ALL` filtered through prompt 24's `contract_id_of` (so `RunReadSkill` is
   absent and the ten built-ins are present), union `ContractId::skill(&summary.name)` version 1 for
   every entry of `SkillCatalog::summaries()`. Do not invent an id, do not enumerate a contract the hub
   does not compile, and do not list a skill that failed a load check.
2. **Paging.** `CatalogPage { entries, next_page }` over the sorted vector: `start = page * page_size`,
   the slice `start..min(start + page_size, len)`, and `next_page = Some(page + 1)` only when more
   entries remain. A `page` past the end returns an empty `entries` and `next_page: None` — that is not
   an error, and the hub treats it as the end of the walk. The handler never reorders between pages: it
   rebuilds the same sorted list on every call, so a concurrent skill reload can only produce a hash
   mismatch, never a silently different set.
3. **Descriptor.** `serde_json::json!({})` for every entry. The hub ignores the field
   (`CatalogEntry.descriptor`, section 6.4), and filling it would be an agent describing its own
   authority.
4. **`hello.catalog_hash`.** `ring`/`sha2` over the canonical text
   `for (id, version) in entries: format!("{id}\t{version}\n")` in the `BTreeMap`'s order, lower-case
   hex, 64 characters, carried through prompt 20's `Sha256Bytes`. Build the entry list **once** in the
   handshake and once per `get_catalog` call from the same live catalogue, and compute the hash from
   the identical list each time. Compute it in `domovoi-core` (`catalog_hash`) so the agent and a hub
   test cannot disagree about the encoding.
5. **Failure.** A catalogue that cannot be read (the same error paths `summaries()` already has) is not
   served as a short list: the handler answers the request with `Message::Error`, the page fails, and
   the hub marks the walk `Partial`. Never serve a partial list under the full hash.
6. **Compatibility.** The agent's `hello.catalog_hash` is `None` only when no contract exists at all,
   which cannot happen: the ten built-ins are compiled in. A 1.2 agent built before this prompt answers
   `unsupported` and is reported as `Partial(unsupported)`; that is the backward case section 9
   describes, and it resolves when the agent is upgraded, not by the hub guessing.

### 6.5 Skill floor persistence **fixed**

```rust
// crates/domovoi-hub/src/capability/floors.rs (new)
/// The raise-only effect-class floors, one settings key per host.
pub struct Floors { /* store, map: Mutex<BTreeMap<HostId, BTreeMap<SkillName, SkillFloor>>>, updates: tokio::sync::Mutex<()> */ }

impl Floors {
    /// Reads `host.<name>.floors` for each host, dropping malformed entries with a warning.
    pub async fn load(store: Arc<dyn SettingsRepo>, hosts: &[Host]) -> Result<Floors, StoreError>;
    /// The floor for `skill`, or `None`.
    pub fn get(&self, host: HostId, skill: &SkillName) -> Option<SkillFloor>;
    /// Writes a new floor. `Some(floor)` must be **stricter** than the current effective class;
    /// `None` clears the floor back to the declared class. `read-modify-write` under the update lock.
    pub async fn set(&self, host: &Host, skill: SkillName, declared: EffectClass,
                     floor: Option<EffectClass>) -> Result<(), FloorWriteError>;
    /// The stored names, for the API and the tests.
    pub fn names(&self, host: HostId) -> BTreeMap<SkillName, EffectClass>;
}
```

- Settings key `host.<name>.floors`, built by a `pub fn host_floors_key(host: &str) -> String`, exactly
  like `host_skills_key` (`skill_policy.rs:26-29`).
- Stored value: `{"floors": {"<skill>": "<read|mutate|destructive>"}}`. Only the three class words prompt
  24 accepts for a `SkillFloor`; `contain` is rejected by `SkillFloor::new` and `FloorError` is surfaced as
  a validation error, never stored.
- **Raise-only on write.** `set` takes `declared` (the class the host's `hello.skills` reports for that
  skill) and refuses (`FloorWriteError::WouldLower`) when the new class is below it or below the current
  floor. `HubRegistry::set_floor` looks `declared` up first and passes it; `Floors` never reads the wire
  itself. `None` clears the floor back to `declared`, which is not a lowering.
- **Read validation.** `load` walks the JSON object: a key that is not a `SkillName`, a value that is not a
  class word, `contain`, and a non-object `floors` are each dropped with one `tracing::warn!` and treated as
  **absent**. A malformed entry is never interpreted as a lower class, and never fails startup.
- Missing key, missing skill, or an empty object → `None`. A floor that was cleared writes an empty object
  (`{"floors": {}}`) rather than deleting the key, the same shape the enablement key writes when a host is
  switched back on (`SkillPolicy::set` always writes the whole JSON, `skill_policy.rs:101-108`). Keep the
  floors key present-but-empty so the UI can show "no floors set" without special-casing `None`.
- One update is one whole step: compute the new map, write the settings value, then publish it in memory.
  A store error leaves both the store and the memory as they were (the pattern at
  `skill_policy.rs:93-109` and its test at `:424-449`).
- The floors feed `skill_contract(summary, floor)` and `effective_effect(declared, floor)` from prompt 24.
  This prompt adds no second computation of the effective class.

No new table, no migration, no column.

### 6.6 Enablement: `host.<name>.skills` stays exactly as it is **fixed**

- Same key (`host.<name>.skills`), same value (`{"disabled": ["name", ..]}`), same JSON, same audit kinds
  (`skill.enabled` / `skill.disabled`), same endpoint (`PUT /api/v1/hosts/{name}/skills/{skill}`,
  `set_skill_enabled`), same 204/400/404 responses, same behaviour for a name the agent did not announce
  (`settings_write.rs:455-463`).
- Same in-memory rule: a host with no disabled names drops out of the map.
- Same `DISABLED_REASON` text: `"the skill is disabled on this host"`.
- **One source of truth.** `Enablement` is the only reader and writer. The registry reads it; the API reads
  it through the registry; nothing else may parse the key. Grep for `host_skills_key` afterwards: one
  definition, one loader, one writer, plus that host's old call in the hub tests.
- **Enforcement stays at the final boundary**, not only in the inventory: `collect_evidence` (for
  `ReadToolCall::RunReadSkill`) and `execute` (before `run_skill`) both ask `registry.enabled(host,
  &ContractId::skill(&skill))` and return `LinkError::Rejected(DISABLED_REASON)`/`EvidenceError::NotEnabled`
  with that exact text. `PolicyLink`'s three narrowed methods (`skill_policy.rs:161-212`) are replaced
  one-for-one, so nothing that used to be refused starts being allowed:
  `run_tool`/`RunReadSkill` → `collect_evidence`; `run_skill` → `execute`; `skills()` → `enabled_skills`
  at the four engine call sites and the `/skills` page's `enabled` flag.

### 6.7 Ceiling: wire summary → `CeilingSummary` **fixed**

```rust
// crates/domovoi-hub/src/capability/ceiling.rs (new)
/// Maps the agent's `hello.ceiling_summary` (a JSON string, wire 1.2) into prompt 24's types.
pub fn parse_ceiling(summary: Option<&str>) -> CeilingParse;

pub enum CeilingParse {
    /// No summary was reported: the agent has no ceiling, or predates it.
    Absent,
    /// A summary arrived and is valid.
    Parsed(CeilingSummary),
    /// A summary arrived and is not usable. Never treated as permissive.
    Malformed { reason: String },
}

/// Whether the ceiling covers this contract id (exact ids only, D-18).
pub fn covers(ceiling: &CeilingSummary, contract: &ContractId) -> bool;
```

Rules:

1. `None`, `""`, `null` → `Absent`. `[unattended].enabled = false` inside a valid summary is also
   `Absent` for the purpose of section 6.1 row 7.
2. Deserialisation goes through prompt 24's `CeilingSummary` (`Deserialize` is derived there). A parse
   error, an unknown contract id inside an entry, a target set that fails `TargetName`, or a negative
   rate is `Malformed`: the registry then reports `NotAllowedOnHost(CeilingAbsent)` for every non-`Read`
   contract. **Unknown or absent ceiling → `NotAllowedOnHost`, never `Available`.**
3. Only `Read` contracts bypass the ceiling. Today's reads keep working whether or not a ceiling exists,
   and the inventory says so.
4. The ceiling hash (`hello.ceiling_hash`) is opaque: the registry compares it for change detection and
   never interprets it.
5. Entries match by **exact contract id**. No wildcard, no prefix, no category: D-18 says "exact
   capability id", and `CeilingEntry.contract` is a `ContractId`.
6. The hub never writes, widens or caches a ceiling into the database. It is agent-owned and read-only
   here; the read-only ceiling view is prompt 49's.
7. Because the ceiling ships with prompt 45, `hello.ceiling_summary` is `None` for every agent in this
   milestone. Consequence, stated plainly: **every non-`Read` contract in the index has no entry**, so
   the inventory shows `NotAllowedOnHost(CeilingAbsent)` for them until an agent reports a ceiling. That is
   the honest answer to "can this host run it unattended", and it does not stop a human-approved action,
   because section 6.1 says the boundaries do not refuse on this state.

### 6.8 `NotAllowedOnHost` and `Unavailable` from the host's own status **fixed**

| Wire input | Value | Effect on availability |
|---|---|---|
| `runs_as_root` | `true` (today's shipped unit) | **A label only** (`AvailabilityEntry.root_agent = true`). Skills stay `Available`. Never `Unavailable`, because that would claim the working labelled-root path is broken |
| `runs_as_root` | `false` | No effect by itself; it only enables the runner rule below and it is the value prompt 45 uses for the root hard limit |
| `skill_runner_status` | `Ok { .. }` | Skills keep whatever rule 6.1 reached; `Available` |
| `skill_runner_status` | `Absent` **and** `runs_as_root == true` | Skills `Available`: the agent runs them itself, as it does today. Nothing is claimed about the runner |
| `skill_runner_status` | `Absent` **and** `runs_as_root == false` | Skills `Unavailable(SkillRunnerMissing)`: an unprivileged agent with no runner has nowhere to run them |
| `skill_runner_status` | `Unsupported { .. }` / `Error { .. }` **and** `runs_as_root == false` | `Unavailable(SkillRunnerUnsupported)` / `Unavailable(SkillRunnerError)` |
| `skill_runner_status` | `Unsupported`/`Error` **and** `runs_as_root == true` | Skills stay `Available` with `root_agent = true`; the runner problem is a diagnostic sentence, because the root path still works |
| `helper_status` | any value | No effect in M2: no contract in the index has `ExecutionPath::Helper`. When prompt 47 adds one, `Absent` → `Unavailable(HelperMissing)`, `Error` → `Unavailable(HelperError)`, `Ok { ops }` → available only for a contract whose id is in `ops` |
| `DockerReport.available` | `false` | Container-targeted contracts get `Unavailable(ContainerRuntimeUnavailable)`; container resolution returns `TargetError::HostUnavailable` |
| `allow_risk` | never on the wire | `NotAllowedOnHost(AllowRisk)` is defined and **never produced in M2**; do not invent a wire field and do not guess from the manifest `risk` |

Two paragraphs, because they are the places where a small model can get this wrong:

- **Never claim helper or skill-runner operations work before H1.** No string in the UI, the API, the book
  or a doc comment may say a privileged operation is "available", "ready" or "enabled" because a helper
  status is `Ok`. In M2 a helper status changes nothing at all, and the inventory shows it as a diagnostic
  fact about the agent, not as capability availability.
- **`runs_as_root` is not a refusal.** D-29 is not in effect; the shipped unit runs the agent as root
  (`03 RW-39`, FACT). Making it `Unavailable` would report every skill on every real host as broken, and
  04 §12 calls it "root-install labelling". The hard limit that actually uses it (unattended execution on a
  root agent → `RequireApproval`) is prompt 39's, in 04 §7.3.

## 7. Ordered implementation steps

Each step is small enough to finish and test on its own. Do not start a step whose predecessor is red.

1. **Verify the repository context.** Open every file in section 3, confirm the seven `SkillPolicy` tests
   and the four engine `link.skills(host)` call sites, confirm `AgentLink` has no catalogue method,
   confirm prompt 20's `GetCatalog`/`CatalogPage` exist in `domovoi-proto`, confirm prompt 24's
   `capability` module and `ContractIndex`. Write every difference down before editing.
2. **Core types, part 1.** `capability/availability.rs`: `Availability`, `NotAllowedOnHost`,
   `UnavailableReason`, `AvailabilityEntry`, `code()`, `reason_code()`, `describe()`, `is_available()`.
   Unit tests for the codes and the `ALL` array.
3. **Core types, part 2.** `capability/catalog.rs`: `CatalogQuery`, `CatalogPageData`,
   `CatalogImplementation`, `CatalogueState`, `CatalogueGap`, `CatalogSnapshot`. Serde round-trip test.
4. **Core types, part 3.** `capability/target.rs`: `ContainerId`, `TargetQuery`, `ResolvedTarget`,
   `TargetError`, `TargetUnresolved`, `validate_container_name`, and the `ContainerId` grammars.
   Unit tests for every accept and reject case.
5. **Core ports and the agent catalogue.** `ports/capability.rs` (`CapabilityRegistry`) and
   `ports/target.rs` (`TargetResolver`); re-export both from `ports/mod.rs`; add the `get_catalog`
   default method to `AgentLink`; add `catalog_hash` to `capability/catalog.rs`.
   `cargo nextest run -p domovoi-core`.
6. **Agent: the catalogue.** `requests/catalog.rs` (`catalog_entries`, `page`), the `GetCatalog` arm in
   `requests/mod.rs`, and `hello.catalog_hash` in `link/handshake.rs`, with the four tests of 6.4.1.
   `cargo nextest run -p domovoi-agent`.
7. **Hub: enablement.** Move `skill_policy.rs` into `capability/enablement.rs` verbatim apart from the
   type name, keep `DISABLED_REASON` and `host_skills_key`, move the seven tests unchanged. Delete the old
   module and its exports.
8. **Hub: floors.** `capability/floors.rs` with load, get, set, raise-only enforcement and the malformed
   handling; unit tests with `InMemoryStore`.
9. **Hub: ceiling.** `capability/ceiling.rs` with `parse_ceiling` and `covers`; tests with a valid summary
   built from prompt 24's types and four malformed ones.
10. **Hub: catalogue.** `capability/catalogue.rs`: the walk, the state machine of section 6.4, the cache;
    `WsAgentLink::get_catalog`; `FakeAgentLink` scripted pages. Tests for every gap variant.
11. **Hub: inventory.** `capability/inventory.rs` (`parse_ps`, `parse_inspect_id`) with their parser tests.
12. **Hub: the registry.** `capability/registry.rs` implementing the port with the precedence table of
    section 6.1, `inventory(host)`, `unknown_contracts`, `set_floor`. Wire it into `runtime/mod.rs`,
    replacing `SkillPolicy`/`PolicyLink`; pass the raw `server.link()` to the engine.
13. **Engine: the boundaries.** `Evidence::new(link, registry)` and the enablement check inside
    `collect_evidence`; the enablement check in `execute`; the four `skills()` call sites through
    `enabled_skills`. Keep every existing error text.
14. **Engine: targets.** `targets/mod.rs` (`TargetService`) over `Evidence`, with the grammar check first,
    the container identity from the full id, and the unit refusal. Wire it in `runtime/mod.rs` after
    `Evidence`.
15. **API.** `api/capability.rs` (`list_capabilities`), `ApiDeps.capabilities`, `hosts.rs`'s `enabled`
    flag, `settings_write.rs`'s switch; `cargo xtask openapi` then `cd web && npm run gen:api`.
16. **Web.** `useCapabilities`, the vocabulary row, the icon, nav/page/route, the read-only page and its
    tests. Then `npm run lint`, `npm run typecheck`, `npm test`, `npm run build`.
17. **Documentation** (section 14) and the completion report at the end of this brief.

## 8. Persistence and migration

- **No migration. No new table, column or index.** `crates/domovoi-store/migrations/` ends at
  `0010_llm_admission.sql` in this checkout; this prompt adds nothing to it.
- Two settings keys, both in the existing `settings` table through `SettingsRepo`:

| Key | Value | Writer |
|---|---|---|
| `host.<name>.skills` | `{"disabled": ["<skill>", ..]}` — unchanged since it shipped | `Enablement::set` via `PUT /hosts/{name}/skills/{skill}` |
| `host.<name>.floors` | `{"floors": {"<skill>": "<read|mutate|destructive>"}}` — new | `Floors::set` (section 6.5) |

- **Atomic update rules.** For each key: take the per-service `tokio::sync::Mutex<()>` (`updates`), compute
  the whole new value from the in-memory map plus this one change, `store.set(key, &value)`, and only then
  publish it in memory. Never read-modify-write without the lock (two switches on one host would lose one:
  `skill_policy.rs:392-421` is the test that pins this). Use `SettingsRepo::set`, not `set_many`: one key
  is one value, and the settings service's other keys are not involved.
- **Backwards compatibility.** An existing `host.<name>.skills` value loads unchanged, including the
  "empty means absent" case and names that are not valid `SkillName`s (ignored, as
  `skill_policy.rs:63-66` does). No key is renamed, no value is migrated, and a hub downgrade finds the
  keys it expects. `host.<name>.floors` is absent on every existing install and reads as "no floors".
- **No new state in the hub's in-memory registry survives a restart**, and none needs to: availability is
  recomputed from the store, the agent's next `hello` and the catalogue walk. A catalogue snapshot is
  memory only.

## 9. Protocol and compatibility

- **Consume 1.2 only.** This prompt reads fields prompt 20 already froze (`hello.catalog_hash`,
  `hello.ceiling_summary`, `hello.ceiling_hash`, `hello.skill_runner_status`, `hello.helper_status`,
  `hello.runs_as_root`) and sends one message prompt 20 already froze (`get_catalog`). No field is added,
  renamed or re-encoded, and `PROTOCOL_VERSION` does not change.
- **No new message, no version change, no `Open<T>` change.** `Unsupported { type_name }` is what a peer
  that does not know `get_catalog` answers; the registry treats it as `Partial(unsupported)`.
- **A 1.1 agent.** Under D-20/`04 §12` the hub refuses a mismatched minor at the handshake, so this case
  must not be reached: the host is `HostIncompatible` and every contract on it says so, from
  `HostAvailability::Incompatible`. Do not add a fallback that reads a 1.1 catalogue.
- **An incompatible agent.** Same: `HostIncompatible { reported }`, and the inventory links to the upgrade
  advice (`04 §12`). Nothing is written by the registry for such a host, and no `get_catalog` is attempted.
- **A 1.2 agent that does not answer `get_catalog`** (a build that predates this prompt): `unsupported`
  or a timeout, catalogue `Partial`, agent-executed contracts `Unavailable(CatalogueIncomplete)`. Reads
  keep working (section 6.1). This prompt ships the answering handler (§6.4.1), so the case disappears
  when the agent is upgraded; do not add a hub-side fallback list.
- **Missing-field rules are prompt 20's**, not this prompt's: `entry_script_digest` is required in 1.2 and a
  summary without it does not decode, so a skill summary that reaches this code always has one. Do not
  add tolerances here.
- **`domovoi-proto` and `domovoi-store` are unchanged.** `domovoi-agent` gains a handler for a message 1.2
  already defines and fills a field 1.2 already carries; neither the message nor the version changes, so a
  hub and an agent of this same version interoperate either way.

## 10. Security requirements

| Requirement | How it is met | Test |
|---|---|---|
| **Agent data never defines authority** | contracts come only from the compiled `ContractIndex`; `CapabilityContract` has no `Deserialize` (prompt 24); the catalogue contributes ids and versions only, and its descriptor is ignored | `agent_catalogue_cannot_create_a_contract` |
| **Unknown risk is `Destructive`** | delegated entirely to prompt 24's `from_declared_risk`/`effective_effect`; this prompt never parses a risk word itself | `effective_effect_comes_from_contracts_only` |
| **Floor raise-only** | `Floors::set` refuses `WouldLower` against the declared class and the current floor; `SkillFloor::new` refuses `Contain`; load drops malformed entries instead of lowering | `floor_cannot_lower_declared_or_current_class`, `malformed_floor_is_treated_as_absent` |
| **No `Contain` from skills** | no code here constructs an effect class; a `contain` floor word is a validation error and is never stored | `floor_rejects_contain` |
| **Disable enforced at the boundary** | `collect_evidence`, `execute` and `enabled_skills`; not only the inventory | `disabled_capability_refused_at_boundary`, `disabled_skill_hidden_from_enabled_skills` |
| **Unknown contract ids ignored** | an id outside the index goes to `unknown`, is never a row and can never be `Available` | `unknown_contract_is_recorded_and_never_offered` |
| **Failed or partial catalogue fails closed** | `NotInstalled` requires `Complete`; a partial snapshot can only produce `Unavailable`; a malformed page fails the whole walk | `partial_catalogue_is_never_available` |
| **No secrets** | nothing here touches a secret; the ceiling summary and status fields are not secrets; the redactor is not bypassed because no text is rendered from agent input except ids and names, which are validated | `capability_dto_contains_no_secret_source` |
| **Nothing can approve or execute** | no `HumanApproval`, `Authorization`, `PolicyPermit`, `expected_safety` or `Contain` construction; the only agent calls are `get_catalog` and the two reads inside `collect_evidence` | `capability_module_constructs_no_authority` (source gate) |
| **No container write path (D-24)** | resolution reads `docker_ps` and `docker_inspect` only; nothing calls a mutating Docker operation | gate: no new `run_skill`/`run_tool` call site outside the boundaries |
| **Container name grammar before resolution** | `validate_container_name` runs first and is anchored | `container_name_grammar_enforced` |
| **Identity changes are refused, not absorbed** | identity is the full id; a new id or a vanished name is `Unresolved`; nothing rewrites a bound identity | `target_identity_change_refused` |
| **No optimistic unit identity** | units always `Unresolved(UnitIdentityUnproven)` while R-8 is open | `unit_identity_is_never_claimed` |
| **No second execution path** | the registry never dispatches; `AgentLink::run_skill` and `run_tool` keep their single call sites | `cargo xtask arch-check` plus prompt 23's gate |

## 11. Error and recovery behaviour

| Situation | Behaviour |
|---|---|
| A catalogue page fails (`Timeout`, `Transport`, `Rejected`, malformed entry) | snapshot `Partial(PageFailed{page})`/`Partial(PageMalformed{page})`; the previous snapshot is discarded, not merged; agent-executed contracts `Unavailable(CatalogueIncomplete)`; one `tracing::warn!` with host and page |
| `next_page` repeats or goes backwards | `Partial(CursorNotAdvancing{page})`; the walk stops; nothing loops |
| More than 50 pages | `Partial(TooManyPages)`; the walk stops; the inventory says the catalogue is too large to verify |
| The announced hash and the walked pages disagree | `Partial(HashMismatch)` → `Unavailable(CatalogueHashMismatch)`; never `Available` |
| The announced hash changed after the walk | cached snapshot `Stale`; the next `catalog()` re-walks; until it finishes, `Unavailable(CatalogueStale)` |
| Disconnect mid-catalogue | `Partial(Disconnected{after_page})`; `HostAbsent`/`HostUnknown` then outranks every contract state |
| Unknown contract id in a page | recorded in `unknown` and in the diagnostic list; one `tracing::info!`; no row, never `Available`, never an error page |
| Malformed settings value (`host.<name>.floors`) | the whole entry is dropped with a warning and treated as absent; startup succeeds; the other entries still load |
| Malformed settings value (`host.<name>.skills`) | existing behaviour is kept: invalid names are filtered out and the rest load (`skill_policy.rs:63-66`) |
| Malformed ceiling summary | `CeilingParse::Malformed`; non-`Read` contracts `NotAllowedOnHost(CeilingAbsent)`; one warning; never a permissive default |
| Container name fails the grammar | `TargetError::InvalidName`; no agent call is made at all |
| Container name resolves to nothing | `TargetError::Unresolved(ContainerVanished)`; no fallback to the name |
| Container inspect has no valid full id | `TargetError::Unresolved(ContainerIdentityUnproven)` |
| Two containers with one name | `TargetError::Ambiguous`; nothing is chosen |
| Unit target | `TargetError::Unresolved(UnitIdentityUnproven)`, always (R-8 open) |
| Store write fails (`Enablement::set`, `Floors::set`) | the error is returned; memory and store are unchanged; the API answers with `describeError`'s text (`StoreError`) and the switch visibly stays where it was |
| Host reconnects with a different catalogue hash | the snapshot is replaced on the next walk; entries computed from the old snapshot are never served as `Complete`; `unknown` is rebuilt, not appended to |
| Host disconnects while the inventory page is open | the next poll shows `HostUnknown`/`HostAbsent` per contract; the page keeps the last rows readable and says the host is offline |
| A skill disappears from `hello.skills` after a restart | it becomes `NotInstalled` (its contract row stays, because the index is compiled); a stored floor for it stays in the settings and is ignored until the skill returns |

## 12. Testing requirements

Fakes come from `domovoi-testkit` (`FakeAgentLink` with `set_presence`/`set_skills`/`requests`,
`InMemoryStore` with `fail_next`, `build::host`, `build::manifest`, and the new
`FakeCapabilityRegistry`). No test sleeps; anything time-dependent uses the fake clock.

### 12.1 Core

| Test | Setup, action, expected |
|---|---|
| `availability_has_eight_states_with_codes` | `Availability::ALL.len() == 8`; every `code()` is one of the eight snake_case strings and unique |
| `availability_reasons_have_codes` | each `UnavailableReason` and `NotAllowedOnHost` yields its documented `reason_code()` |
| `container_name_grammar_enforced` | accept `plex`, `my-container`, `a.b_c`, `Plex`; reject ``, `p`, `-plex`, `_plex`, `.plex`, `plex `, ` plex`, `plex/1`, `a b`, 129 chars |
| `container_id_requires_64_lowercase_hex` | accept 64 hex chars; reject a 12-character short id, uppercase, 63, 65, empty, `0x`-prefixed |
| `catalogue_state_serde_round_trip` | `Complete`, three `Partial` gaps and `Stale` round-trip |
| `target_query_host_identity_shape` | `host:{HostId}` parses as a `TargetIdentity` and carries the fingerprint field |
| `unit_identity_is_never_claimed` | `TargetQuery::Unit` construction is fine; the resolver contract asserts `Unresolved(UnitIdentityUnproven)` (implemented in the engine test below; here: the variant's `describe()` says identity is unproven) |

### 12.2 Hub registry

| Test | Setup, action, expected |
|---|---|
| `availability_is_host_unknown_before_any_session` | `FakeAgentLink` with no presence; every contract → `HostUnknown` |
| `availability_is_host_absent_after_a_session_ends` | presence connected then disconnected with `last_seen`; → `HostAbsent { since }` |
| `availability_is_host_incompatible_for_another_protocol` | `LinkError::Incompatible` path / `HostAvailability::Incompatible { reported }`; → `HostIncompatible { reported }` |
| `available_contract_is_available` | connected, complete catalogue naming `container.logs`, no ceiling needed for `Read`; → `Available` |
| `not_installed_requires_a_complete_catalogue` | complete catalogue without `container.inspect`; → `NotInstalled`; and with a partial catalogue the same contract is `Unavailable(CatalogueIncomplete)`, never `NotInstalled` |
| `disabled_skill_is_disabled_by_hub` | `host.<name>.skills` disables `prune_logs`; `skill.prune_logs` → `DisabledByHub`, `skill.zpool_status` → `Available` |
| `mutating_contract_without_a_ceiling_is_not_allowed_on_host` | a crafted `Mutate` contract in the index, `ceiling_summary` absent; → `NotAllowedOnHost(CeilingAbsent)`; a `Read` contract on the same host → `Available` |
| `ceiling_entry_must_match_the_exact_contract` | summary with an entry for `container.restart`; a different `Mutate` id → `NotAllowedOnHost(CeilingEntryMissing)` |
| `unprivileged_agent_without_a_runner_cannot_run_skills` | `runs_as_root = false`, `skill_runner_status = Absent`; `skill.x` → `Unavailable(SkillRunnerMissing)` |
| `root_agent_without_a_runner_keeps_skills_available` | `runs_as_root = true`, runner `Absent`; `skill.x` → `Available`, `root_agent == true` |
| `helper_status_changes_nothing_in_this_milestone` | `helper_status = Absent`/`Error`; every contract's state is identical to the run without it |
| `container_runtime_unavailable_is_reported` | `DockerReport.available = false`; a container-targeted contract → `Unavailable(ContainerRuntimeUnavailable)` |
| `precedence_host_state_outranks_everything` | host `Absent` **and** a disabled skill **and** a partial catalogue: skill → `HostAbsent` |
| `precedence_disable_outranks_not_installed` | a disabled skill missing from `hello.skills` → `DisabledByHub` |
| `precedence_not_installed_outranks_ceiling` | missing from a complete catalogue and no ceiling → `NotInstalled` |
| `partial_catalogue_is_never_available` | `Partial` in each of its gaps; no agent-executed contract is `Available` |
| `unknown_contract_is_recorded_and_never_offered` | page names `container.destroy`; `unknown_contracts(host)` contains it; no `AvailabilityEntry` has that id |
| `skill_contract_comes_from_hello_skills` | a skill in a catalogue page but not in `hello.skills` is unknown/diagnostic, not a row; a skill in `hello.skills` only is `Available` |
| `hub_executed_contracts_ignore_the_catalogue` | `incident.history` is `Available` with a `Partial` catalogue |
| `agent_catalogue_cannot_create_a_contract` | pages name ids outside the index; `contracts().len()` is unchanged |
| `disabled_capability_refused_at_boundary` | registry with `prune_logs` disabled; `collect_evidence` for `RunReadSkill{prune_logs}` and `execute` for `prune_logs` both return the refusal carrying `DISABLED_REASON`, and `FakeAgentLink::requests()` is empty |
| `disabled_skill_hidden_from_enabled_skills` | `enabled_skills(host)` omits the disabled name and keeps the agent's own list at two (via `AgentLink::skills`) |
| `effective_effect_comes_from_contracts_only` | floor `Destructive` on a `Read` skill → `effective_effect` is `Destructive`; no floor → the declared class |
| `floor_cannot_lower_declared_or_current_class` | raise `Read`→`Mutate` ok; `Mutate`→`Read` → `WouldLower`; `Destructive`→`Mutate` → `WouldLower`; stored value unchanged after a refusal |
| `floor_rejects_contain` | `set_floor(.., Contain)` → validation error; nothing stored |
| `malformed_floor_is_treated_as_absent` | stored `{"floors": {"x": "contain", "y": "nonsense", "z": 3, "ok_skill": "mutate"}}`; load keeps only `ok_skill`; one warning per dropped entry |
| `floor_survives_a_reload` | set, reload `Floors::load`, same map |
| `two_floor_writes_on_one_host_keep_both` | `tokio::join!` of two `set` calls; the stored JSON has both (the `skill_policy.rs:392-421` pattern) |
| `store_that_refuses_a_floor_write_leaves_floors_unchanged` | `InMemoryStore::fail_next`; memory and store unchanged |
| `malformed_ceiling_is_not_permissive` | five malformed summaries; all → `NotAllowedOnHost(CeilingAbsent)` for a `Mutate` contract |
| `ceiling_hash_is_opaque` | two summaries with different hashes and the same entries produce the same coverage answer |
| `catalogue_walk_stops_on_a_repeating_cursor` | page 0 → next 0; `Partial(CursorNotAdvancing)`, two requests at most |
| `catalogue_walk_bounds_the_page_count` | 51 pages scripted; `Partial(TooManyPages)`; exactly 50 requests |
| `catalogue_hash_mismatch_is_partial` | announced hash `aa..`, walk completes, snapshot `Partial`; `Unavailable(CatalogueHashMismatch)` |
| `catalogue_reconnect_with_a_new_hash_replaces_the_snapshot` | walk with hash A, then a new session announcing B; `catalog()` walks again, entries are B's, `unknown` is rebuilt |
| `catalogue_walk_uses_one_deadline` | fake clock advanced past 30 s between pages; `Partial(Deadline)`; at most two pages requested |
| `container_name_is_validated_before_any_agent_call` | `resolve_target(Container{name:"plex/1"})` → `InvalidName`; `requests()` is empty |
| `target_identity_change_refused` | resolve `plex` → identity bound to id `aa…`; a later `docker_ps`+`docker_inspect` reports id `bb…` (recreate); resolving yields a **different** identity, and a comparison against the bound one reports a change rather than matching; a rename with the same id yields the **same** identity |
| `ambiguous_container_name_is_not_chosen` | two inspect results for one name → `Ambiguous` |
| `vanished_container_is_unresolved` | name absent from `docker_ps` → `Unresolved(ContainerVanished)` |
| `unit_target_is_unresolved` | `resolve_target(Unit{..})` → `Unresolved(UnitIdentityUnproven)`; nothing was requested from the agent |
| `host_target_carries_the_enrolled_key_fingerprint` | store has an agent; `resolve_target(Host{..})` → `host:{id}` plus a 64-hex fingerprint |
| `parse_ps_reads_the_agent_table` | the exact header and three rows; three names, order preserved; a line with no tab is skipped, not fatal |
| `parse_inspect_id_reads_the_full_id` | a real Docker inspect document (fixture with `"Id"`); the id; a document without `Id` → error |

The six target-resolution rows from `container_name_is_validated_before_any_agent_call` to
`host_target_carries_the_enrolled_key_fingerprint` are **engine** tests, in
`crates/domovoi-engine/tests/services/targets.rs` (registered in `tests/services/main.rs` like its
neighbours), because `TargetService` lives in the engine and needs `Evidence`. Every one of them drives a
scripted `FakeAgentLink` and asserts that the only calls it saw were `docker_ps`/`docker_inspect` through
`collect_evidence` — never `run_tool` from the test.

### 12.3 Hub API and web

| Test | Setup, action, expected |
|---|---|
| `capability_dto_covers_every_field` | `GET /api/v1/hosts/nas/capabilities` → every row has contract, title, category, effect, state, catalogue, target kind, execution, `root_agent`; `state` is one of the eight codes |
| `capability_endpoint_requires_auth` | no session → 401 |
| `capability_endpoint_404s_on_an_unknown_host` | `GET /hosts/nope/capabilities` → 404 |
| `capability_endpoint_reports_unknown_contracts` | a page naming an unknown id → it appears in `unknown_contracts`, not in `entries` |
| `skills_switch_still_works` | the existing `crates/domovoi-hub/tests/hub/skills.rs` suite passes unchanged |
| web: loading | `useCapabilities` pending → `Loading` from the kit, no layout shift |
| web: empty | a host with no rows → `EmptyState` with a real sentence |
| web: error | 500 → `LoadError` with `describeError`'s text and a working retry |
| web: offline host | `HostAbsent` rows still render; the page says the host is offline instead of showing an empty table |
| web: partial catalogue | the page shows the catalogue as unverified and never renders `Available` |
| web: unknown future availability value | a `state` of `"quarantined"` renders through `lookup`'s fallback: the raw text, tone `neutral`, the "not known to this version of the UI" hint, no crash |
| web: axe | the page test runs `axe` over what it leaves on screen, in both themes, and there is no violation |

`crates/domovoi-hub/tests/gates.rs` is **extended**, never weakened: a source scan proves
`crates/domovoi-hub/src/capability/` contains no `HumanApproval`, `PolicyPermit`, `Authorization`,
`expected_safety` or `Contain` construction, and that `AgentLink::run_skill` has no second call site.

### 12.4 Agent catalogue (section 6.4.1)

In `crates/domovoi-agent/src/requests/catalog.rs`'s test module, plus one hub test.

| Test | Setup, action, expected |
|---|---|
| `catalog_lists_every_contract_bearing_read_tool` | a catalogue with no skills: exactly the ten `contract_id_of` ids, version 1, `skill.`-free, `run_read_skill` absent |
| `catalog_lists_every_loaded_skill` | three loaded skills: `skill.<name>` version 1 for each, in id order, and no skill that failed a load check |
| `catalog_pages_are_deterministic_and_end_with_none` | 250 entries, `page_size = 100`: pages 0, 1, 2 with 100/100/50 entries, `next_page` `Some(1)`, `Some(2)`, `None`; repeating a call returns the identical slice |
| `catalog_page_beyond_the_end_is_empty_not_an_error` | `page = 9`: empty `entries`, `next_page: None`, no error |
| `catalog_page_size_is_clamped` | `page_size = 0` → 100; `page_size = 1000` → 100 |
| `catalog_hash_matches_the_served_entries` | the `catalog_hash` the agent reports equals `catalog_hash(&catalog_entries(..))`, and re-walking the pages through a `FakeAgentLink` gives the same map |
| `catalog_hash_changes_when_a_skill_appears` | same catalogue plus one skill → a different hash; the rest of the list unchanged |

The hub-side half is a single test in `crates/domovoi-hub/tests/hub/capability.rs`:
`a_real_agent_catalogue_walks_to_complete` — build the agent's list with the real function (the agent crate
is not a hub dev-dependency, so copy the two-line list construction, not the code under test), script it
through `FakeAgentLink`, and assert the walk ends `Complete` with the same hash. It proves the two sides
agree on the encoding; the per-side equality tests above prove each side alone.

## 13. Acceptance criteria

1. `domovoi_core::capability` gains `Availability` (eight variants), `AvailabilityEntry`,
   `CatalogueState`, `CatalogSnapshot`, `TargetQuery`, `ResolvedTarget`, `TargetError`, `ContainerId` and
   `validate_container_name`, all documented, with prompt 24's items unchanged.
2. `domovoi_core::ports::CapabilityRegistry` exists with the six methods of section 6.2,
   `domovoi_core::ports::TargetResolver` exists with its one method, and `AgentLink::get_catalog` exists
   with a default that fails closed.
3. The agent answers `get_catalog` from the list of `contract_id_of` ids plus the loaded skills, pages it
   deterministically, and reports the matching `hello.catalog_hash`; a skill appearing or disappearing
   changes that hash (section 6.4.1).
4. `HubRegistry` answers the precedence table of section 6.1 in that exact order, proven by the four
   `precedence_*` tests.
5. `PolicyLink` and `AgentServer`'s `skill_policy` module are gone; grep finds no `PolicyLink` and exactly
   one `host.<name>.skills` reader.
6. `collect_evidence` and `execute` both refuse a disabled skill with `DISABLED_REASON`, and the four
   engine `link.skills` call sites use `enabled_skills`.
7. `host.<name>.skills` and its endpoint behave exactly as before; `crates/domovoi-hub/tests/hub/skills.rs`
   passes unchanged.
8. `host.<name>.floors` stores the JSON of section 6.5, refuses a lowering write and a `contain` class, and
   treats malformed entries as absent.
9. A ceiling that is absent, disabled or has no exact entry for a non-`Read` contract yields
   `NotAllowedOnHost`, never `Available`; `Read` contracts are unaffected.
10. A catalogue that is incomplete, has a cursor problem, a duplicate, an unannounced or mismatched hash, or
    was cut by a disconnect yields `Partial`/`Stale` and never `Available` or `NotInstalled`.
11. An id the hub does not know is recorded in `unknown_contracts` and never appears as a row.
12. Container targets resolve by full id after the grammar check; a recreate changes the identity, a
    rename does not; units are always `Unresolved(UnitIdentityUnproven)`. The resolver is the engine's
    `TargetService` behind `domovoi_core::ports::TargetResolver`, and its reads go through `collect_evidence`.
13. `GET /api/v1/hosts/{name}/capabilities` serves the read-only inventory; `web/src/api/openapi.json` and
    `web/src/api/schema.d.ts` are regenerated, and the DTO contains no secret.
14. `/capabilities` is reachable from the `Fleet` group and renders the inventory in both themes; `/skills`
    and its switch are unchanged.
15. No migration, no new table, no new crate, no new protocol message, no protocol version change.
16. `cargo fmt --all`, `cargo clippy -p domovoi-core -p domovoi-hub -p domovoi-engine -p domovoi-agent
    -p domovoi-testkit --all-targets -- -D warnings`, `cargo nextest run -p domovoi-core -p domovoi-hub
    -p domovoi-agent -p domovoi-testkit`, `cargo nextest run -E 'rdeps(domovoi-core)'`,
    `cargo xtask arch-check` and `cargo xtask docs-links` are green.
17. The new tests exist under the names in section 12 and pass.

## 14. Documentation requirements

1. **`ARCHITECTURE.md` — this prompt owns the M2 update under D-15** ("`ARCHITECTURE.md` updated once per
   milestone; each prompt names its sections"). Update exactly these sections, verified in this checkout:
   - **§5.6 Skills and actions** (`ARCHITECTURE.md:156`): contracts are hub-owned; a skill is the
     `skill.<name>` family with a raise-only floor; the hub switch is unchanged.
   - **§6 Ports** (`:186`): add one row for `CapabilityRegistry` — purpose, implemented by hub (and
     testkit), used by engine and hub.
   - **§7.1 Hub** (`:206`): the capability registry replaces the per-host link wrapper (`PolicyLink`); the
     registry evaluates no policy.
   - **§10 HTTP API and web UI** (`:278`): the read-only `/api/v1/hosts/{name}/capabilities` endpoint and
     the `/capabilities` page.
   - **§12 Skills and actions** (`:297`): enablement and availability are enforced at the execution and
     read boundaries, and `HostAvailability`/`Availability` are distinct.
   Do **not** change §4 (no new crate) or §8 (the wire is already 1.2 and unchanged).
2. **Book.** There is no `docs/book/src/**/capabilities*` page today (checked: `docs/book/src` has
   `concepts/actions.md`, `guide/skills.md` and no capability page). Create
   `docs/book/src/concepts/capabilities.md` — what a capability is, that contracts are hub-owned, what the
   eight states mean in plain language (one short paragraph each), that "available" is not "approved", and
   that the M2 page is read-only. Add `- [Capabilities](concepts/capabilities.md)` to
   `docs/book/src/SUMMARY.md` under `# Concepts` (after "Actions and approvals").
3. **`docs/book/src/guide/skills.md`.** Its paragraph about the **Enabled** switch (`:57`) stays true;
   add one sentence that the switch is per host, that the capability inventory shows the same state, and
   that a raised floor can only make a skill stricter.
4. **`docs/book/src/reference/api.md`.** Add the new endpoint in the hosts section, in the same shape as
   its neighbours.
5. **OpenAPI.** `cargo xtask openapi`, then `cd web && npm run gen:api`. Both generated files are
   committed; never hand-edit them.
6. **Doc comments.** Every new public item says what it is for. Module docs on
   `capability/mod.rs` (hub) explain in five to ten lines: contracts are compiled in, the catalogue is the
   agent's implementation list, enablement and the floor are the only hub-side switches, and nothing here
   approves.
7. **No prompt numbers, plan references or agent references** in any product document, doc comment, UI
   string, commit message or PR description. Say "the capability registry", not "prompt 25".

## Execution constraints

You are implementing **one** change in the Domovoi repository (a Rust workspace with an embedded React UI).
This document is your complete brief. The planning documents it cites are background: what you need is
written here.

1. **Inspect before relying on anything.** Before you edit, open every file, type and function this
   brief names.
   - Confirm that each one exists and behaves as described.
   - If something differs materially, write down the difference **before** changing code. Examples: a
     different signature, a missing module, behaviour already implemented, or a prerequisite prompt not
     merged.
   - If the difference changes what you would build, stop and report it instead of guessing.
2. **Follow the contracts here exactly.** Do not change public interfaces, wire formats, database
   schemas or security behaviour beyond what this brief specifies.
3. **Stay in scope.** No unrelated refactoring, renaming or reformatting. No new dependencies unless
   this brief names them. Add any new dependency to `[workspace.dependencies]` in the root
   `Cargo.toml` and reference it from crates with `{ workspace = true }`.
4. **Never weaken a security check, a gate test or an arch rule to make something pass.**
5. **Stop and report rather than invent.** Do this when the brief contradicts the code, an
   architectural decision is missing, or a prerequisite does not exist.
6. **Be truthful.**
   - Never claim a test passed unless you ran it and saw it pass.
   - Never present stubbed or partial work as complete.
   - Mark anything unfinished as unfinished.

## Repository rules

These come from `AGENTS.md`, which CI enforces.

- **Crate boundaries.** `domovoi-core`, `domovoi-proto` and `domovoi-engine` do no I/O. Only
  `domovoi-runner` spawns processes, always with argv vectors and never `sh -c`. Dependency edges follow
  `ARCHITECTURE.md` §4. `cargo xtask arch-check` enforces all of this.
- **Approvals.** `HumanApproval::new` is called only in `crates/domovoi-hub/src/auth/`, and
  `HumanApproval::restore` only in `crates/domovoi-store/src/`. The LLM tool catalogue stays closed.
  Nothing but an authenticated human approves an action.
- **Code rules.**
  - No `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!` or `dbg!` in library code.
  - No new `#[allow(...)]`.
  - `thiserror` in libraries; `anyhow` only in `domovoi` and `xtask`.
  - Every public item has a doc comment saying what it is for.
  - Non-test source files stay at or under 500 lines; web source files at or under 250 lines.
- **Data and time.** Never edit a merged migration; add a new one. Choose a migration's number by listing
  `crates/domovoi-store/migrations/` on your branch and taking the next unused number; never assume it
  from a plan or a predecessor. Renumber only your own unmerged file if the branch gained a migration
  first, and describe the migration by its contents, not its number. Time comes from the `Clock` port;
  tests advance the fake clock from `domovoi-testkit` and never sleep. Secrets are
  `secrecy::SecretString` and are never logged, put into prompts or returned from the API.
- **Web.** Read `web/DESIGN.md` before touching `web/`. Build only from the kit in
  `web/src/components/`. Use `useActionFeedback` for every mutation and `describeError` for every error
  message, and tokens only for styling.
- **Tests.** Name tests after the behaviour, not the function. Never delete or weaken `tests/gates.rs`.
  Review every `insta` snapshot you accept.
- **Branch and commits.**
  - Work on a branch named as the brief says.
  - Commit messages are one line in Conventional Commits form, with exactly one scope, for example
    `feat(store): ...`. No body and no trailers.
  - Committed files, commit messages and the PR description must not mention prompts, prompt numbers,
    the build plan, coding agents or AI assistance.
- **Checks to run.** Run them for what you changed only. CI runs the full suite.
  - `cargo fmt --all`
  - `cargo clippy -p <crate> --all-targets -- -D warnings` for each crate you edited
  - `cargo nextest run -p <crate>` for each crate you edited, or `cargo test -p <crate>` if nextest
    is not installed
  - `cargo nextest run -E 'rdeps(<crate>)'` when you changed a public item that other crates use
  - `cargo xtask arch-check` when you added or moved modules or touched a `Cargo.toml`
  - for `web/`: `npm run lint`, `npm run typecheck`, and the vitest files for the changed area
  - On Windows, the hub and agent run only under WSL 2; unit and integration tests run natively.

## Completion report

End with a report in exactly this structure:

1. **Implemented behaviour**: what now works, in observable terms.
2. **Files**: added, changed and removed, each with one line on why.
3. **Implementation decisions**: any choice the brief left to repository convention, and what you chose.
4. **Interfaces**: types, functions, endpoints and messages introduced or changed, with signatures.
5. **Database and protocol changes**: migrations (file names), schema changes, wire changes.
6. **Security**: how each security requirement in the brief is met, and where it is tested.
7. **Tests added**: the test names, and which acceptance criterion each covers.
8. **Commands run**: each exact command and its actual result (pass, fail, counts).
9. **Not run**: tests or checks you did not run, and why.
10. **Deviations**: every place you departed from the brief, and why.
11. **Known limitations and remaining work**.
