# 42 — Packages: hub import, store, relay and install jobs

| | |
|---|---|
| Branch | `packages-hub` |
| Milestone | P1 |
| Depends on (must be merged) | 29 (use-case layer; see "Hub use-case layer" below), 41 (`packages-agent`: `domovoi_core::package::*`, `package_contracts()`, the agent receiver and backend, `PackageFixture`), 25 (capability registry; **written but NOT merged when this was written**, and 24 with it), 22 and 23 (merged: `Authorization`, plan hash v2, the capability-object boundary). 41 itself was written but is not merged yet: every `domovoi_core::package` name below is **introduced by prompt 41, verify names in code before use**. |
| Can run in parallel with | 26–40 (order-flexible, never concurrent). 49 (`capability-center-and-install-wizard`) must run **after** this prompt. |
| Produces (shared contracts) | `domovoi_core::install::*` (artifact, job, target, status, failure, inventory types); `domovoi_core::action::PackagePlan` (the package plan hash); `AgentLink::send_transfer`, `start_capability`, `capability_result` and the `PackageRelay` capability object (`PackageCaps`); hub `[packages]` config; tables `package_artifacts`, `install_jobs`, `install_job_targets`, `host_packages`; `StateWrite::Install`; `domovoi_engine::packages::{InstallService, PackageHandle}`; the `/api/v1/packages/*` and `/api/v1/hosts/{name}/packages*` endpoints and DTOs; the `domovoi package import` and `domovoi package list` commands; two new arch rules and their ADR |
| Consumes | wire 1.2 `transfer_*`, `run_capability`, `capability_result`, `refused`, `action_status` (frozen, **not changed here**); 41's agent contract (frame order, failure codes, output keys); 25's registry (`CapabilityRegistry::availability`, `ContractIndex`); 22's `Authorization`, `ExecutionId`, `KeyFingerprint`; 23's capability objects and EXEC rules; the unit of work and audit chain (08, 09); the `HostReconciler` hook (21) |
| Launch gate | none beyond the dependencies |
| Suggested model | medium (security-sensitive wiring; every decision is made below, follow the contracts exactly) |


## Hub use-case layer (read first)

Prompt 29 (`use-case-layer`) runs before this prompt in the recommended order and changes how hub code is
organised. Check `crates/domovoi-hub/src/services/` on your branch. If it exists:

- Every command (anything that writes) is a method on a service in `crate::services`, taking `&Caller`.
  The HTTP handler in `api/` and any chat handler in `auth/interactions/` only parse the request, call that
  method and map the result. Plain reads may stay in `api/` handlers.
- Do not add an engine handle (`ActionHandle`, `OperationHandle` or a new service handle of yours) to
  `ApiDeps` or `ChatDeps`; give it to `Services` instead. The arch rules `MOD-1`…`MOD-4` reject the
  other form.
- A new top-level hub module must be listed in `ZONES` or `UNZONED_MODULES` in `xtask/src/arch/rules.rs`.
- Where this brief says "add `X` to `ApiDeps`" for a handle used by a command, read it as "add `X` to
  `Services`". Read-only dependencies may still go on `ApiDeps`.

If `services/` does not exist, prompt 29 is not merged: stop and report, because the order is wrong.

## 1. Objective

Give the owner a way to **import a signed package, keep it on the hub, and ask chosen hosts to install or remove
it**, with the hub acting only as a store and a courier. Concretely, on the hub side only:

1. **Owner import.** Upload (HTTP multipart) or `domovoi package import <archive> <sig>` (CLI) of a
   `<name>-<version>.tar.zst` and its detached `<name>-<version>.sig.toml`. The hub checks the signature file
   shape, the digest, and the signature against a **hub-side key list the owner edits in `hub.toml`**, then stores
   the pair in a content-addressed package store and records it in the package index. The hub never opens the
   archive.
2. **Package index.** List, read and delete imported packages; show which hosts have what installed.
3. **Install and uninstall jobs.** An owner request, authorized by a human with a step-up (password and typed
   confirmation), creates one job with one **target per host**. A worker relays the artifact and the signature file
   to each host over the frozen 1.2 `transfer_*` frames, then sends `run_capability` for `agent.package.install` or
   `agent.package.uninstall` and settles the result. Partial success per host, retry per host.
4. **Crash recovery.** A hub restart in any window settles or reconciles every target; an install whose outcome the
   hub cannot prove becomes `outcome_unknown` after the agent's journal is consulted (prompt 21's mechanism).
5. **Audit.** Every step is an audit row in the same transaction as the state change.
6. **API surface for prompt 49.** The endpoints and DTOs in section 6.9. No UI here.

It does **not** change the wire, the agent, the skill runner, the enabling of skills, or anything under `web/`
except the two generated API files.

## 2. Architectural context

**Who is trusted (RW-26, D-11, D-16).** The **agent** is the only authority over what is installed on its host. It
verifies a package against keys the owner edited into that host's `agent.toml` (the first-party release key and
owner keys, minus a revocation list). There is no third-party registry in v1 and the hub **cannot add, replace or
remove a key on any agent**: no wire field, capability argument, setting or API in this prompt carries one. The hub
may *offer* a package; a malicious or compromised hub can offer packages and withhold installs, nothing more.

**Why the hub still verifies (and why that is not authority).** The hub keeps a **hub-side key list**
`[[packages.owner_keys]]` in `hub.toml`, edited by the owner in the file, never through the API. It is used for two
things only: (a) refusing, at import time, a package nobody could install (unknown key, revoked key, bad signature,
digest not matching the signature file), so the store holds only well-formed signed material; (b) showing the
signer label in the UI. The agent re-verifies everything and ignores this list. A key being in the hub list does
not make any agent trust it; the owner must also put it in each agent's `[[packages.trusted_keys]]`. This is how
04 §11.2's test `owner_import_requires_trusted_owner_key` is satisfied without giving the hub trust authority.

**Install never enables, never authorizes (RW-26, 04 §11.2).** "Installed" means the agent holds verified files.
"Enabled" is a separate step-up action and is out of scope here; package skills are not runnable until the skill
runner exists (prompt 47). Everything this prompt reports about a host is "installed", never "enabled".

**Jobs, not Operation steps (decision, from 04 §2 and §11.2, RW-26, and prompt 26's non-goals).** 04 §2 lists
"Package install job: owner = install service, table `install_jobs`" as its own aggregate beside operations, and
says the install service is a request/response service that commits through the same `UnitOfWork` with optimistic
versions. 04 §11.2 describes the hub role as "store, relay, track install jobs; one job per host, per-host progress,
partial success per host, retry per host". Prompt 26 states that operations execute only `skill.<name>` contracts
and that `run_capability` has no caller. So a package install is a **standalone job aggregate**, started by an
owner's explicit request, never proposed by the model and never a step of an Operation. It is nevertheless routed
through the **single-execution-boundary discipline of prompt 23 / ADR 0023**: the only code that may hand a
package request to an agent is one function (`execute_package`) in the engine, which takes a value that can only
be built from a stored **human** `Authorization`; the engine holds a narrow capability object (`PackageRelay`),
never the link; and `xtask arch-check` counts the raw link operations. A policy authorization is refused
(`NotYetSupported`), as everywhere in M1, and 41's agent also refuses `authorization_kind = policy` for install and
uninstall.

**Why one more plan hash.** 22's plan hash v2 needs a `PlanTarget` that carries an incident's `CheckKey`; a package
request has no incident. Prompt 41 states the agent does not recompute the hash and that the hub's boundary owns
it. So this prompt adds `PackagePlan` (a different domain line, `plan:package-v1`, so it can never collide with
`plan:v1`/`plan:v2`) binding exactly what the owner is shown: contract and version, package name, version, archive
digest, signer key id, host id and the host's enrolled key fingerprint. The approval is bound to it
(`approvals bound to the displayed plan hash`, 04 §8).

**Data flow.**

```
owner (web, step-up)                  hub                                             agent (prompt 41)
 POST /packages/import  ──▶ stream to temp file, sha256, verify sig with hub key ─▶ package store (files) + index (DB)
 POST /packages/jobs/plan ─▶ plan per host: plan_hash, availability, installed version   (no write)
 POST /packages/jobs ──▶ password + typed name + plan_hash recheck ──▶ ONE transaction:
                            install_jobs + install_job_targets(queued) + Authorization(human) + audit
 worker: queued ─▶ transferring ──────────────────────────────────────────────────────────────────────────
         transfer_begin/chunk*/end (artifact id A) ───────────────────────────────▶ receiver ─ transfer_result{Stored}
         transfer_begin/chunk*/end (signature id B, kind package_manifest) ───────▶ receiver ─ transfer_result{Stored}
         ─▶ dispatched (commit BEFORE sending: session, journal epoch, transfer ids)
         run_capability{agent.package.install, args{package,version,artifact_transfer=A,signature_transfer=B}} ─▶ backend
         ◀ capability_result{succeeded|failed} ◀──────────────────────────────────  journal finished
         ─▶ succeeded | failed(<PackageFailureCode>) | outcome_unknown (reconcile via action_status)
```

**Frame facts the hub must respect (from 41, fixed).** Two transfers, **artifact first, then the signature file**;
each is `transfer_begin`, sequential `transfer_chunk` (`seq` from 0, gap-free, at most 64 KiB of raw bytes each),
`transfer_end`; the agent answers each transfer with exactly **one** `transfer_result`, as the reply to the
`transfer_end` envelope (the hub correlates by the id of the `end` frame it sent). `TransferKind::PackageArtifact`
carries the `.tar.zst` bytes; `TransferKind::PackageManifest` carries the **detached `.sig.toml` bytes**. There is
no chunk acknowledgement and no abort frame; a failure is reported once, at `end`. A missing `transfer_result` is a
failed transfer, never a success. Both transfer ids are fresh UUIDs per attempt.

## 3. Repository context

Verified in this checkout (branch `overhaul-investigation`, HEAD `74764a3`). Re-open each before editing and
report differences. Migrations end at `0011_plan_binding.sql`; take the next free number when you add yours
(prompt 34 may also be waiting for `0012`; read the directory).

**Hub transport (what exists, what is missing).**

- `crates/domovoi-core/src/ports/agent_link.rs`: `trait AgentLink` has `run_tool`, `start_skill`, `snapshot`,
  `skill_result`, `run_check`, `presence`, `availability` (default method), `journal_epoch`, `action_status`
  (`Vec<ActionId>`, `ActionAnswers { epoch, answers }`, `ActionAnswer { Finished{succeeded, detail}, Running,
  NotFound, Unknown{reason} }`), `skills`. **It has no `run_capability`, no transfer method and no catalogue
  method.** `LinkError { NotConnected, Incompatible, SessionChanged, Timeout, Rejected(String),
  Indeterminate(String), Transport(String) }`. Contract of `start_skill`: failing means nothing was queued;
  `skill_result` errors other than `Rejected` are not proof of anything. The package methods copy this contract.
- `crates/domovoi-core/src/ports/caps.rs`: the capability objects (`AvailabilityProbe`, `HostObservation`,
  `HostRead`, `HandoffRunner`, `JournalStatus`, `CheckRerun`) and bundles `ActionCaps`, `EvidenceCaps`. The hub
  turns its one `PolicyLink` into capabilities **once**, in `crates/domovoi-hub/src/runtime/caps.rs`
  (ADR 0023, `docs/adr/0023-capability-objects-one-checker-bounded-evidence.md`).
- `crates/domovoi-hub/src/agent_server/registry.rs`: `Registry::dispatch(host, &Message, expected_session,
  expected_epoch) -> (RequestId, oneshot::Receiver<Message>, SessionFacts)` (`:275-311`). It registers a pending
  reply, caps pending requests at `MAX_PENDING = 256` (`:26`) and queues with **`outbox.try_send`**, answering a
  full queue with a "busy" `LinkError`. `deliver` (`:328`) hands a reply to the waiter by `reply_to`. **A 20 MiB
  artifact is about 320 chunk frames: sending them through `dispatch` would fail with "busy" after 256.** The relay
  therefore needs a new registry path that awaits queue capacity (section 6.4). `Live.outbox` is a bounded `mpsc`
  of encoded frames (`session.rs:30-60`).
- `crates/domovoi-hub/src/agent_server/session.rs:220-232`: replies of kind `TransferResult`, `CapabilityResult`,
  `Refused`, `ActionStatusAnswers`, `Error` are already delivered to whoever waits on `reply_to`.
- `crates/domovoi-hub/src/agent_server/link.rs`: `WsAgentLink` (`:31`), `request`/`request_on` (`:125-170`),
  `start_skill` (`:290`), `skill_result` (`:316`), `PendingRequest` drop guard (`:221`). The two-phase pattern
  (`start_*` queues and stores the receiver in `self.sends()`, `*_result` awaits) is what the capability methods
  copy. `skill_policy.rs:144-195` (`PolicyLink`) forwards each `AgentLink` method; it needs the three new ones.
- `crates/domovoi-proto/src/envelope.rs:11`: `MAX_FRAME_BYTES = 1 << 20`; `bytes.rs:127`: `MAX_BASE64_BYTES = 64 *
  1024`; `messages/transfer.rs`: `TransferBegin/Chunk/End/Result`, `TransferOutcome { Stored, Failed{code, detail}
  }`. `agent_server/ws.rs:16`: `MAX_FRAMES_PER_SECOND = 100` limits frames the **hub receives** from one agent, not
  frames it sends, so chunking is not rate-limited by it.
- `crates/domovoi-hub/src/agent_server/mod.rs:99,207-213`: one `reconciler: OnceLock<Arc<dyn HostReconciler>>`,
  "the first reconciler wins"; `runtime/mod.rs:253` sets it to the action service. `HostReconciler::reconcile_host(
  HostId)` (`ports/reconcile.rs`) is called on each authenticated `hello`. A second reconciler must therefore be
  composed, not set (section 6.7).
- `crates/domovoi-core/src/id.rs:78-107`: `ExecutionId` (UUID newtype; `from_action`, `as_action`), `ActionId`.
  `action_status` takes `ActionId`s that are plain UUIDs on the wire, so `execution.as_action()` is the query key.

**Authorization, plan hash, approvals.**

- `crates/domovoi-core/src/action/authorization.rs`: `Authorization::human(HumanApproval)` (private
  representation, `kind()`, `human_approval()`), `AuthorizationKind { Human, Policy }`. Confined by
  `xtask/src/arch/rules.rs` `AUTHORIZATION_HUMAN_ALLOWED_PREFIXES` (`:184`): hub `auth/` and testkit only.
  `HumanApproval::new` only in `crates/domovoi-hub/src/auth/`; `HumanApproval::restore` only in
  `crates/domovoi-store/src/` (rules `:170-176`). `crates/domovoi-hub/src/auth/approval.rs` has
  `web_approval`, `web_authorization(user, confirm_text, skill, session_hash, now) -> (Authorization,
  ApprovalProvenance)`; `ApprovalChannel::Web { typed_confirmation }`.
- `crates/domovoi-core/src/action/approval.rs`: `PlanHash` (a `sha256_newtype!`; its `digest` is `pub(crate)` to
  `domovoi-core`, so a new hash family must live inside core), `PlanBinding`, `ApprovalProvenance`.
  `crates/domovoi-core/src/action/plan.rs`: `PlanContract::new(id, version)` (grammar `^[a-z][a-z0-9_.-]{0,63}$`,
  so `agent.package.install` is valid), `PlanTarget` (needs a `CheckKey`; not usable here), `PlanHash::compute_v2`.
  `crates/domovoi-core/src/host.rs`: `KeyFingerprint` (of an agent public key) and `host_target_text(host, &key)`.
  `xtask/src/arch/scan.rs:233`: the line `plan:v2` may be written only under `crates/domovoi-core/src/action/`.
- Step-up today: `crates/domovoi-hub/src/settings/step_up.rs` has `STEP_UP_SETTINGS` (enforced) and
  `STEP_UP_ACTIONS` (`:80-103`), whose `installation.start` and `capability.enable` entries say "Nothing enforces
  those yet". The mechanism: the request carries `password`, the handler calls
  `Auth::confirm_password(ip, user_id, password)` (`auth/mod.rs:282`, shares the login lockout), maps
  `SettingsError::PasswordRequired`/wrong to 403 and lockout to 429 (`api/settings_write.rs:150-190`, example
  `update` handler with `ConnectInfo(peer)`, `HeaderMap`, `proxy::client_ip`).
- Actions API example to copy: `crates/domovoi-hub/src/api/actions.rs:104-170` (`Authed(current)`, `ApproveRequest`
  with `plan_hash` and `confirm_text`, `web_authorization`).

**Store, UoW, config, API.**

- `crates/domovoi-core/src/ports/uow/transaction.rs`: `AggregateKind` (`:15`), `AggregateRef`, closed `StateWrite`
  (`:164-202`), `Transaction { writes, journal, audit, new_effects, ... }`, `Expected`, `CommitError`.
  `uow/journal.rs:15`: `JournalKind` (`as_str`, `parse`), `AuditDraft::new(actor, at, kind, subject, detail)`.
  `crates/domovoi-core/src/actor.rs:17`: `Actor::{User, System(String), Agent, Policy}`; `Actor::system("packages")`.
  Prompts 26 and 34 add `AggregateKind`/`JournalKind`/`StateWrite` variants too: **add yours beside theirs**, keep
  theirs. `crates/domovoi-store/src/uow/writes.rs:106-140` is the single `apply` dispatch; the in-memory twin is
  `crates/domovoi-testkit/src/repos/uow_apply.rs`; the crash harness summary is
  `crates/domovoi-testkit/src/crash/mod.rs:101-140` (exhaustive `match`).
- Repo example: `crates/domovoi-store/src/repos/admission.rs`; `repos/mod.rs`; migration style
  `crates/domovoi-store/migrations/0010_llm_admission.sql` (STRICT tables, integer ms timestamps).
- Hub config: `crates/domovoi-config/src/hub/mod.rs:35-84` (`HubConfig`, each section `#[serde(default)]`,
  `deny_unknown_fields`, `JsonSchema`), `hub/retention.rs` as a section example. 41 adds
  `domovoi-config/src/packages.rs` with the **agent's** `PackagesSection`, `TrustedKey`, `PackageLimits` and key
  shape validation: reuse `TrustedKey` and its validation for `owner_keys`; do not duplicate it. The config crate has
  no `base64`/`sha2` (allowed deps are in `xtask/src/arch/rules.rs`), so decoding keys happens in the hub.
- API: routes registered in `crates/domovoi-hub/src/api/mod.rs:330-385` with `routes!(...)`; the multipart import
  precedent is `api/notes_import.rs` (`Multipart`, `DefaultBodyLimit::max` on a merged sub-router at `mod.rs:380-383`,
  router default `middleware::MAX_BODY_BYTES` = 1 MiB at `router.rs:25`). Note that route **buffers** each file with
  `field.bytes()`; this prompt must not (section 6.5). `Authed(current)` gives `current.user.id` and
  `current.session_hash`. Errors: `api/error.rs` `ApiError`, `ErrorBody { error, message, issues }`.
- CLI: `crates/domovoi/src/cmd/audit.rs` is the pattern for offline commands: `HubConfigArg`,
  `OfflineAdmin::open(config, clock)` (`crates/domovoi-hub/src/admin/mod.rs`), which holds the data directory's
  `HubLock` and refuses to run next to a hub.
- Hub dependencies already present: `base64`, `ed25519-dalek`, `sha2`, `toml` (`crates/domovoi-hub/Cargo.toml:34-51`).
  `domovoi-engine` has `sha2`, `toml`; `domovoi-core` has `sha2`, `hex`, `uuid`, `serde*`, `jiff`, no `toml`.
- Arch cards: `xtask/src/arch/rules.rs:203-340` (`RAW_OPS`, `BOUNDARY_PAIRS`, `BOUNDARY_CARDS`, EXEC-1..4); a
  change to `rules.rs` requires a numbered ADR in `docs/adr/` (`xtask/src/arch/git.rs:82-95`); the last ADR is
  `0023`, take the next number.

**Introduced by prompt 41 (verify names in code):** `domovoi_core::package::{PackageName, PackageVersion, KeyId,
ArchiveDigest, signing_message, SignatureFile, SIGNATURE_FORMAT, MAX_SIGNATURE_FILE_BYTES, PackageFailureCode,
keys::*, PackageListEntry, PackageStatus, KeyState, PrivilegeBoundary}`;
`domovoi_core::capability::package_contracts()` (ids `agent.package.install`, `agent.package.uninstall`,
`agent.package.list`, all version 1); `domovoi_testkit::package::{PackageFixture, BuiltPackage}`. **Introduced by 24/25
(verify):** `ContractId`, `ContractIndex`, `CapabilityRegistry::availability(host, &ContractId) -> Availability`
(`Available`, `NotInstalled`, `DisabledByHub`, `NotAllowedOnHost`, `Unavailable`, `HostUnknown`, `HostAbsent`,
`HostIncompatible`), the hub's `ContractIndex` composition (41 already added the package contracts to it).

**Discrepancies found while writing this brief (resolved here; report if the code disagrees).**

1. Roadmap P1 lists a "first-party index" and 05 §5 lists a hub `[packages] registries` key. No document specifies
   the index format, the URL, who signs it, or the first-party key distribution, and D-16 forbids third-party
   registries. **Resolution: owner import only.** No network fetching, no `registries` key, no index URL. The
   artifact table keeps a `source` column whose only value today is `owner_import` so a first-party source can be
   added later without a migration of meaning. Report this to the maintainer.
2. 04 §11.2's diagram shows an "Enabled" state and audit kinds `enabled`/`disabled`. Enabling is a separate action
   outside this prompt (and package skills are not executable before 47).
3. 41 says no document specifies the package build and signing tool. This prompt does not invent one either; tests
   build packages with `PackageFixture`.
4. Prompt 26 and 23 describe `run_capability` as having no caller. The hub gets its first package caller here, with
   new method names (`send_transfer`, `start_capability`, `capability_result`) so the existing `run_capability`
   "no caller" card is left untouched.

## 4. Exact scope

Files to **add** (non-test files at most 500 lines; split rather than exceed):

| File | Purpose |
|---|---|
| `crates/domovoi-core/src/install/{mod,artifact,job,target,status,failure,call,inventory}.rs` | hub-side shared types (6.1) |
| `crates/domovoi-core/src/action/package_plan.rs` | `PackagePlan`, `JobPlanHash` (6.2) |
| `crates/domovoi-core/src/ports/repos/install.rs` | `InstallRepo` read port (6.6) |
| `crates/domovoi-config/src/hub/packages.rs` | hub `[packages]` section and validation (6.8) |
| `crates/domovoi-store/migrations/00NN_install_jobs.sql` | tables (section 8) |
| `crates/domovoi-store/src/repos/install.rs`, `crates/domovoi-store/src/install/{mod,apply,codec}.rs` | `InstallRepo` for SQLite and the `StateWrite::Install` applier |
| `crates/domovoi-engine/src/packages/{mod,service,worker,plan,relay,execute,reconcile,inventory,audit,settle}.rs` | install service and worker (6.5, 6.7) |
| `crates/domovoi-hub/src/packages/{mod,keys,verify,store,import}.rs` | hub key list, signature check, file store, import pipeline (6.4, 6.5) |
| `crates/domovoi-hub/src/api/{packages,packages_jobs,packages_hosts,dto_packages}.rs` | endpoints and DTOs (6.9) |
| `crates/domovoi/src/cmd/package.rs` | `domovoi package import` and `list` (6.10) |
| `docs/adr/00NN-package-relay-boundary.md` | required because `rules.rs` changes |
| tests: `crates/domovoi-engine/src/packages/tests/*`, `crates/domovoi-hub/tests/hub/packages*.rs`, `crates/domovoi-store/tests/install_*.rs`, `crates/domovoi-core/src/install/tests.rs`, `crates/domovoi/tests/package_cli.rs` | section 12 |

Files to **change** (minimal edits): `crates/domovoi-core/src/{lib.rs,action/mod.rs}`, `ports/{agent_link,caps,mod}.rs`,
`ports/repos/mod.rs`, `ports/uow/{transaction,journal,mod}.rs` (new `AggregateKind`, `JournalKind`, `StateWrite`
variants), `crates/domovoi-config/src/hub/{mod,validate}.rs` (+ `lib.rs` re-export, schema snapshot),
`crates/domovoi-store/src/{lib,uow/writes,purge}.rs` and `repos/mod.rs`, `crates/domovoi-engine/src/lib.rs`,
`crates/domovoi-hub/src/{lib,router?}`: `agent_server/{registry,link,skill_policy,mod}.rs`, `runtime/{caps,mod,
schedulers}.rs`, `auth/approval.rs`, `api/{mod,dto?}.rs`, `settings/{step_up,patch}.rs`, `admin/mod.rs`,
`crates/domovoi/src/cmd/mod.rs`, `xtask/src/arch/rules.rs`, `crates/domovoi-testkit/src/{link,repos/*,lib}.rs`,
engine/hub `tests/gates.rs`, `web/src/api/{openapi.json,schema.d.ts}` (generated only), docs (section 15).

## 5. Explicit non-goals

- **No wire change.** Nothing under `crates/domovoi-proto/`; protocol stays 1.2; no golden frame changes.
- **No agent change.** Nothing under `crates/domovoi-agent/`; 41 owns it. No change to the skill runner, helper,
  installer or units.
- **No key authority.** No API, command, setting, wire field or capability argument that adds, replaces or removes a
  trusted key on an agent. `owner_keys` and `revoked` in the hub config are file-only.
- **No third-party or first-party registry fetching**, no `registries` config key, no index URL (discrepancy 1; owner decision D-32).
- **No enabling** of package skills, no change to `host.<name>.skills`, no `SkillCatalog` change.
- **No model involvement.** The LLM tool catalogue stays closed; there is no tool, chat command or button that
  creates a package job. Chat (Discord) cannot approve one: only the web channel with a password can.
- **No Operation integration.** Not a step kind, not an `Operation`; no change to prompt 26's tables.
- **No policy path.** `AuthorizationKind::Policy` is refused; prompt 39/40 must not offer package contracts to
  policy `Allow`.
- **No hub-side archive parsing.** The hub never decompresses, untars or interprets `package.toml`; no `ruzstd`,
  no `tar` in the hub.
- **No UI** (prompt 49), no CLI for installs (install is web-only: it needs a session, password and typed
  confirmation).
- **No version-ordering policy**: any imported version may be installed over another (rollback is a normal
  install; the agent re-verifies against current trust).
- **No background polling of `agent.package.list`** and no periodic refresh job.
- Do not weaken any gate test or arch rule; do not touch merged migrations.

## 6. Required interfaces and contracts

Marked **fixed** (must match) or **convention** (follow repository style).

### 6.1 Core types (`domovoi_core::install`, new, pure; **fixed** names and semantics)

```rust
/// Identifies one imported package artifact (UUIDv7 via `define_id!`).
pub struct PackageArtifactId(Uuid);
/// Identifies one owner request to install or remove a package on hosts.
pub struct InstallJobId(Uuid);
/// Identifies one attempt on one host (a retry is a new target id).
pub struct InstallTargetId(Uuid);

/// Where the artifact came from. Only `OwnerImport` exists in v1.
pub enum ArtifactSource { OwnerImport }                       // as_str "owner_import"

pub struct PackageArtifact {
    pub id: PackageArtifactId,
    pub name: PackageName, pub version: PackageVersion,
    pub archive_sha256: ArchiveDigest, pub key_id: KeyId,
    pub archive_size: u64,                       // bytes of the .tar.zst
    pub signature_toml: String,                  // the exact .sig.toml text, at most MAX_SIGNATURE_FILE_BYTES
    pub signer_label: Option<String>,            // from the hub key list at import time (display only)
    pub source: ArtifactSource,
    pub imported_by: Actor, pub imported_at: Timestamp,
}

pub enum InstallKind { Install, Uninstall }                   // as_str "install" | "uninstall"

pub enum TargetStatus {                                       // as_str snake_case; validated in Rust, never SQL CHECK
    Queued, Transferring, Dispatched,                         // non-terminal
    Succeeded, Failed, NotRun, OutcomeUnknown, Cancelled,     // terminal
}
impl TargetStatus { pub const fn is_terminal(self) -> bool; }

pub struct InstallJob {
    pub id: InstallJobId, pub kind: InstallKind,
    pub name: PackageName, pub version: PackageVersion,
    pub archive_sha256: Option<ArchiveDigest>,   // Some for install; None for uninstall
    pub key_id: Option<KeyId>,                   // same
    pub requested_by: UserId, pub created_at: Timestamp,
    pub job_plan_hash: PlanHash,                 // JobPlanHash (6.2)
}

pub struct InstallTarget {
    pub id: InstallTargetId, pub job: InstallJobId, pub host: HostId, pub attempt: u32,   // attempt starts at 1
    pub execution: ExecutionId,                  // fresh UUIDv7, UNIQUE; the agent journal key
    pub status: TargetStatus,
    pub plan_hash: PlanHash,                     // PackagePlan hash for this host and key
    pub authorization: StoredAuthorization,      // approver, channel, time, provenance; kind is always Human
    pub expires_at: Timestamp,                   // created_at + approval window
    pub artifact_transfer: Option<Uuid>, pub signature_transfer: Option<Uuid>,
    pub dispatched_session: Option<AgentSessionId>, pub dispatched_epoch: Option<JournalEpoch>,
    pub failure: Option<TargetFailure>,          // code + message, for Failed / NotRun / OutcomeUnknown
    pub outputs: Option<BTreeMap<String, String>>, // the agent's capability outputs, when known
    pub outputs_confirmed: bool,                 // false when settled from the journal alone
    pub queued_at: Timestamp, pub started_at: Option<Timestamp>, pub finished_at: Option<Timestamp>,
    pub version: u64,                            // optimistic version
}

/// `{code, message}`; `code` is a snake_case token (6.1 failure list), `message` at most 500 characters.
pub struct TargetFailure { pub code: String, pub message: String }

/// What the hub last learned about one installed package on one host (display only, never a decision input).
pub struct HostPackage {
    pub host: HostId, pub name: PackageName, pub version: PackageVersion, pub key_id: KeyId,
    pub archive_sha256: ArchiveDigest, pub installed_at: Timestamp,
    pub status: PackageStatus, pub key_state: KeyState, pub skills: Vec<String>,
    pub privilege_boundary: PrivilegeBoundary, pub observed_at: Timestamp, pub observed_by: ObservedBy,
}
pub enum ObservedBy { Job, Refresh }                          // as_str "job" | "refresh"
```

`StoredAuthorization` carries what `actions` stores for an approval (user, channel, `approved_at`, optional
`session_ref`, `client`); the store rebuilds the `HumanApproval` with `HumanApproval::restore` and exposes only an
`Authorization` through `StoredAuthorization::authorization()` (called by the store, which is allowed to use
`restore`; `Authorization::human` is **not** callable from the store, so the engine receives the authorization as
`(HumanApproval, ApprovalProvenance)` and never wraps it itself: `AuthorizedPackageTarget::of` takes the restored
`HumanApproval` and checks it, see 6.5. Verify how `crates/domovoi-store/src/repos/actions.rs:59-70` hands the
approval to `ActionRequest::from_stored` and follow it exactly).

Failure codes (**fixed**, stored as text, `message` is the human sentence): hub-originated `host_unavailable`,
`agent_incompatible`, `not_advertised`, `artifact_missing`, `artifact_corrupt`, `transfer_failed`, `session_changed`,
`handoff_failed`, `approval_expired`, `result_mismatch`, `hub_restarted`, `agent_refused`; plus every
`PackageFailureCode::as_str()` value from prompt 41 **passed through verbatim** when the agent reports
`"<code>: <detail>"`; plus the refusal codes `contract_unknown`, `allow_risk_denied`, `authorization_expired`,
`rate_limited`, `journal_corrupt` passed through verbatim. An unrecognised agent message is `agent_refused` with the
text truncated to 500 characters.

Status meaning (**fixed**): `NotRun` = the hub has proof nothing was installed or removed (pre-dispatch failure,
never handed over, journal says `NotFound`). `Failed` = the agent answered that it refused or failed (its work is
atomic: nothing is left half-installed). `OutcomeUnknown` = handed over, no authoritative answer; the owner must
check the host. `Succeeded` = the agent said so and the outputs match, or the journal says finished-ok.

### 6.2 Plan hashes (`domovoi_core::action::package_plan`, **fixed** encoding)

```rust
pub struct PackagePlan { /* contract id+version, subject, host, host key fingerprint */ }
impl PackagePlan {
    pub fn install(name: &PackageName, version: &PackageVersion, digest: &ArchiveDigest, key_id: &KeyId,
                   host: HostId, host_key: &KeyFingerprint) -> Self;
    pub fn uninstall(name: &PackageName, version: &PackageVersion, host: HostId, host_key: &KeyFingerprint) -> Self;
    pub fn list(host: HostId, host_key: &KeyFingerprint) -> Self;
    pub fn hash(&self) -> PlanHash;
}
/// The hash of a whole job: the sorted per-host hashes, so the owner approves the set they were shown.
pub fn job_plan_hash(kind: InstallKind, host_hashes: &[PlanHash]) -> PlanHash;
```

Encoding (length-prefixed like 22's v2 so values cannot make it ambiguous; domain lines are **not** `plan:v2`):

```
"plan:package-v1\ncontract:{len}:{id}@{version}\nsubject:{len}:{subject}\ntarget:{len}:{host_target_text(host,key)}\n"
subject (install)   = "name={name}\nversion={version}\nsha256={digest}\nkey={key_id}"
subject (uninstall) = "name={name}\nversion={version}"
subject (list)      = "none"
job hash = SHA-256("plan:package-job-v1\nkind:{install|uninstall}\n" + each host hash hex sorted ascending, one per line)
```

The execution id is **not** an input (a retry gets a new execution id but must show the same plan). **Retry compares the per-host hash**, never the job hash: `RetryRequest.plan_hash` must equal the recomputed host hash for that one target, and `InstallTargetDto` carries that per-host `plan_hash` so the UI can show and send it. Contract ids:
`agent.package.install`, `agent.package.uninstall`, `agent.package.list`, version `1` (use `PlanContract::new`).

### 6.3 Link additions (`AgentLink`, **fixed** semantics, **convention** shapes)

All three get **default bodies** returning `Err(LinkError::Transport("not supported by this link".into()))` so other
implementations keep compiling; `WsAgentLink`, `PolicyLink` and the testkit `FakeAgentLink` implement them.

```rust
/// One transfer frame, in the order the receiver needs.
pub enum TransferFrame {
    Begin { transfer: Uuid, kind: TransferKind, size: u64, sha256: [u8; 32] },
    Chunk { transfer: Uuid, seq: u64, bytes: Vec<u8> },      // 1..=65_536 bytes
    End   { transfer: Uuid },
}
pub enum TransferKind { PackageArtifact, PackageManifest }   // core mirror of the wire enum
pub enum TransferAnswer { Stored, Failed { code: String, detail: String } }

/// What to ask an agent's package backend. The three contracts and nothing else.
pub enum PackageCall {
    Install { package: PackageName, version: PackageVersion, artifact_transfer: Uuid, signature_transfer: Uuid },
    Uninstall { package: PackageName, version: PackageVersion },
    List,
}
pub struct PackageDispatch { pub execution: ExecutionId, pub call: PackageCall,
    pub plan_hash: PlanHash, pub expires_at: Timestamp }
pub struct CapabilityHandoff { /* RequestId, like SkillHandoff */ }
pub struct PackageOutputs(pub BTreeMap<String, String>);

// on trait AgentLink
async fn send_transfer(&self, host: HostId, frame: TransferFrame, expected_session: AgentSessionId,
                       deadline: Timestamp) -> Result<Option<TransferAnswer>, LinkError>;   // Some only for End
async fn start_capability(&self, host: HostId, req: PackageDispatch, expected_session: AgentSessionId,
                          expected_epoch: JournalEpoch, deadline: Timestamp) -> Result<CapabilityHandoff, LinkError>;
async fn capability_result(&self, host: HostId, handoff: CapabilityHandoff, deadline: Timestamp)
                          -> Result<PackageOutputs, LinkError>;
```

Semantics (**fixed**):

- `send_transfer` for `Begin` and `Chunk` returns once the encoded frame is **in the session's outgoing queue**
  (it must wait for queue room, bounded by `deadline` on the `Clock`; it must **never** use `try_send` and must not
  register a pending reply). For `End` it queues the frame, registers a pending reply, and waits for the matching
  `TransferResult` (`reply_to` = the `End` frame's id), returning `Some(Stored | Failed{..})`. Frames of one
  transfer are queued strictly in order by one caller; every call passes `expected_session` and fails with
  `SessionChanged` if the live session is not exactly that one (checked under the same lock as the queueing). A
  `Refused` or `Error` reply to `End` is `LinkError::Rejected("<code>: <detail>")`. A timeout or dropped session
  while waiting for `End` is `Indeterminate` (the transfer may have been stored; the hub treats the target as not
  run and starts a new transfer id on retry).
- `start_capability` builds `RunCapability { execution_id, contract, version: 1, target, args, plan_hash,
  authorization_kind: Human, expires_at, expected_safety: None }`. `contract` and `args` come from the closed
  `PackageCall` (arg names are the `domovoi_core::package::keys::ARG_*` constants; **no other key can be sent**).
  `target` is `package:<name>` for install/uninstall and `packages:all` for list (verify `TargetKey::parse`
  accepts both; `domovoi-core/src/signal/keys.rs:82`). Failing means nothing was queued (same contract as
  `start_skill`).
- `capability_result`: `CapabilityOutcome::Succeeded{outputs}` -> `Ok(PackageOutputs)`;
  `Failed{code, message}` -> `Err(Rejected(message))` (the agent already writes `"<PackageFailureCode>: <detail>"`
  in `message`); `Refused` -> `Err(Rejected("<code>: <detail>"))`; an `Error` frame `in_progress` or
  `outcome_unknown` -> `Err(Indeterminate(..))`; anything else, a timeout or a dropped session -> `Indeterminate`
  or `Timeout` (never proof of anything).
- `authorization_kind` is always `Human` for the three package contracts. For `List` this is truthful only because
  a list is requested by an authenticated human action (the refresh button or a job the human authorized): there is
  **no background caller** of `List` (non-goal).

### 6.4 The capability object and the registry path

```rust
/// Core `ports/caps.rs`, next to the others. The only thing the engine's package service may use to reach agents.
#[async_trait] pub trait PackageRelay: Send + Sync {
    async fn transfer(&self, host: HostId, frame: TransferFrame, session: AgentSessionId, deadline: Timestamp)
        -> Result<Option<TransferAnswer>, LinkError>;
    async fn hand_off(&self, host: HostId, req: PackageDispatch, session: AgentSessionId, epoch: JournalEpoch,
        deadline: Timestamp) -> Result<CapabilityHandoff, LinkError>;
    async fn await_result(&self, host: HostId, handoff: CapabilityHandoff, deadline: Timestamp)
        -> Result<PackageOutputs, LinkError>;
}
#[derive(Clone)] pub struct PackageCaps {
    pub probe: Arc<dyn AvailabilityProbe>, pub observer: Arc<dyn HostObservation>,
    pub journal: Arc<dyn JournalStatus>, pub relay: Arc<dyn PackageRelay>,
}
```

`crates/domovoi-hub/src/runtime/caps.rs` builds `PackageCaps` from the same `PolicyLink` it already uses (one more
adapter next to `ActionCaps`; the only two call sites of each raw package operation are `skill_policy.rs` and
`runtime/caps.rs`, mirroring `start_skill`). Registry additions (**convention**): `Registry::dispatch_reserved(host,
frame, expected_session) -> Result<Reservation, LinkError>` that clones the live `outbox` sender under the lock
after the session check, releases the lock, then `reserve().await`s capacity racing the deadline on the `Clock`,
and sends the already-encoded frame; and a variant that also registers the pending reply for `End`. The lock is
never held across an await.

### 6.5 Engine: `domovoi_engine::packages` (**fixed** behaviour)

```rust
pub struct InstallService { /* deps, worker handle */ }
#[derive(Clone)] pub struct PackageHandle { /* commands */ }
impl PackageHandle {
    pub async fn plan(&self, req: PlanRequest) -> Result<InstallPlan, PackageError>;          // read only
    pub async fn create_job(&self, req: CreateJob, auth: HumanApproval, prov: ApprovalProvenance)
        -> Result<InstallJobView, PackageError>;
    pub async fn retry_target(&self, job: InstallJobId, host: HostId, plan: PlanHash,
        auth: HumanApproval, prov: ApprovalProvenance) -> Result<InstallJobView, PackageError>;
    pub async fn cancel_target(&self, job: InstallJobId, host: HostId, actor: UserId) -> Result<InstallJobView, PackageError>;
    pub async fn refresh_inventory(&self, host: HostId, actor: UserId) -> Result<Vec<HostPackage>, PackageError>;
}
```

`PlanRequest { kind, name, version, hosts: Vec<HostId> }` (1 to 16 hosts, no duplicates). `InstallPlan` has, per
host: host id and name, `availability` (the registry's answer for the install or uninstall contract), what the hub
last saw installed (`HostPackage`), the `plan_hash`, and `warnings`; and the `job_plan_hash`. `create_job` recomputes
every per-host plan from **current** data and refuses with `PlanChanged` unless `job_plan_hash` equals the one the
caller submitted, so a re-enrolled host key, a replaced artifact or a changed signer between preview and submit
invalidates the approval.

**The one execution function (the package boundary, **fixed**).**

```rust
// crates/domovoi-engine/src/packages/execute.rs
pub struct AuthorizedPackageTarget { /* target: InstallTarget, approval: HumanApproval */ }
impl AuthorizedPackageTarget {
    /// The only constructor; `pub(crate)`, called once, in worker.rs. Refuses a target whose stored
    /// authorization is not a human approval.
    pub(crate) fn of(target: InstallTarget, approval: Option<HumanApproval>) -> Result<Self, PackageExecError>;
}
pub(crate) async fn execute_package(shared: &Shared, auth: AuthorizedPackageTarget, job: InstallJob,
    artifact: Option<PackageArtifactFile>) -> Result<TargetOutcome, PackageExecError>;
pub enum PackageExecError { NotAuthorized, NotYetSupported, Expired, NotAdvertised, HostUnavailable, ... }
```

`execute_package` is the only caller of `PackageRelay::{transfer, hand_off, await_result}`, takes the authorized
value by value, never stores it, and never builds an `Authorization`. The worker (the only caller of
`AuthorizedPackageTarget::of` and of `execute_package`) drives the target through the states below. All state
changes are `StateWrite::Install` writes committed through the `UnitOfWork` with an audit row and a journal row in the
same transaction, guarded by the previous status (compare-and-set on `status` and `version`), so a stale worker or a
double run changes nothing.

Per-target procedure (**fixed** order; each numbered commit is one transaction):

```
claim(target):        queued ──commit(1)──▶ transferring        // only if no other non-terminal target for that host
  1. approval check: stored authorization is Human (else NotYetSupported/NotAuthorized, settle not_run)
     now < expires_at else settle not_run{approval_expired}
  2. registry availability for the contract (install|uninstall) must be Available
       HostUnknown|HostAbsent  -> keep queued (retry on wake) until expires_at, then not_run{approval_expired}
       HostIncompatible        -> not_run{agent_incompatible}
       NotInstalled|NotAllowedOnHost|DisabledByHub|Unavailable -> not_run{not_advertised} with the reason text
  3. snapshot = observer.observe_snapshot(host)  (session, journal_epoch); none -> not_run{host_unavailable}
  4. install only: open the artifact from the package store; stream it once through SHA-256 while sending;
     file missing -> not_run{artifact_missing}; digest != index -> not_run{artifact_corrupt}
     (the digest of the bytes SENT is also computed and must equal the announced one; see 6.5.1)
  5. artifact transfer A:  Begin{A, PackageArtifact, size, sha256}, Chunk{A, 0..n}, End{A} -> must be Stored
     signature transfer B: Begin{B, PackageManifest, len, sha256(sig bytes)}, Chunk*, End{B} -> must be Stored
     any Failed / Rejected / Indeterminate / SessionChanged / Timeout -> not_run{transfer_failed|session_changed}
     (agent detail kept, truncated to 500 characters); commit after both: record transfer ids (no state change)
     audit package.relayed after B is stored
  6. re-check: session unchanged (snapshot.session == recorded), host still Known, now < expires_at
  7. dispatch commit(2): transferring ──▶ dispatched, store session, journal_epoch, transfer ids, started_at
     BEFORE the request is sent; audit package.install_dispatched
  8. hand_off(...)  Err  -> proof nothing was handed over: settle not_run{handoff_failed}   (commit 3a)
                    Ok   -> await_result(deadline = now + contract timeout + 30 s)
  9. settle (commit 3):  Ok(outputs)            -> verify outputs (6.5.2) -> succeeded (+ inventory) | outcome_unknown{result_mismatch}
                         Err(Rejected(msg))     -> failed{code from msg} (agent said no; nothing installed)
                         Err(Timeout|Indeterminate|Transport) -> stay dispatched; reconcile (section 10)
```

Uninstall skips steps 4 and 5 (no transfers). Contract timeouts are the agent contract's: install 300 s, uninstall
60 s, list 30 s. Per-transfer deadline = `60 s + size / (256 KiB/s)`, at most 15 min (**convention** constant).

**6.5.1 Bytes sent are the bytes indexed.** The announced `sha256` in `Begin` is the index digest; the worker hashes
the file while chunking and compares at the end of the loop **before** sending `End`; a mismatch is
`artifact_corrupt` and `End` is never sent (the agent drops the half transfer by its idle timeout). Chunk size is
exactly 65 536 bytes except the last.

**6.5.2 Output verification.** For install success the outputs must contain `package`, `version`, `key_id`,
`archive_sha256` (keys from `domovoi_core::package::keys`) equal to the job's values and `enabled == "false"`. Any
difference is `OutcomeUnknown{result_mismatch}` and audit `package.install_mismatch` (a real discrepancy the owner
must look at), never `Succeeded`. For uninstall success the outputs `package`/`version` must match; `removed` may be
`"true"` or `"false"` (both mean the package is absent).

**6.5.3 Concurrency.** One non-terminal target per host (also a partial unique index, section 8); at most 4 hosts
relaying at once; targets of one host run in `queued_at` order. The worker wakes on a `Notify` (job created, host
authenticated, retry) and on a `Clock` timer every 30 s for targets waiting for a host; tests drive it with the fake
clock and never sleep.

**6.5.4 Inventory.** After a succeeded install the engine upserts `host_packages` from the outputs
(`observed_by = job`, `status = active`, `key_state = trusted`, skills from `OUT_SKILLS`); after an uninstall it
deletes the row. `refresh_inventory` sends `PackageCall::List` through the same relay (`hand_off` +
`await_result`, deadline 30 s), parses `OUT_PACKAGES` (a JSON `Vec<PackageListEntry>`), replaces the host's rows in
one transaction, and writes audit `package.inventory_refreshed`, plus `package.key_untrusted_detected` for each entry
whose status is now `disabled_untrusted_key` and was not before. The list call has its own `ExecutionId` and
`PackagePlan::list`. The refresh is human-initiated (authenticated session), not automatic.

### 6.6 Core ports and the unit of work (**fixed** names, **convention** field shapes)

- `AggregateKind::InstallJob` (`as_str` `"install_job"`, `AggregateRef::install_job(id)`); `JournalKind::InstallTarget`
  (`"install.target"`, `payload_v` 1, payload `{target, host, attempt, from, to, code?}`); both with `parse`
  round-trip. Beside the variants 26 and 34 add; do not renumber theirs.
- `StateWrite::Install(InstallWrite)` with `InstallWrite` (all guarded):
  `ImportArtifact(PackageArtifact)` (insert; conflict on `(name, version)` with another digest is
  `CommitError::Constraint`), `DeleteArtifact { id, expected: ArtifactGuard }` (refused while a non-terminal target
  references it), `CreateJob { job, targets }`, `Target { target, expected: TargetGuard { status, version } }`,
  `Inventory { host, entries, replace: bool }`. `TargetGuard` compares the stored `status` and `version`; a mismatch
  is `CommitError::Conflict`.
- `InstallRepo` (read): `artifacts()`, `artifact(name, version)`, `artifact_by_digest`, `job(id)`, `jobs(filter,
  page)`, `targets(job)`, `target(id)`, `non_terminal_targets()`, `queued_targets()`, `latest_target(job, host)`,
  `inventory(host)`, `hosts_with(name, version)`. Pagination follows `Page`/`next_cursor` as `ActionRepo::list`.
- The in-memory twin and the crash-harness summary get a match arm each (the compiler lists them; fix every
  exhaustive `match` on the three extended enums).

### 6.7 Reconciliation hook (**fixed**)

`InstallService` implements `HostReconciler`. `runtime/mod.rs` currently does `server.set_reconciler(Arc::new(
actions_service.clone()))` and the server keeps only the first reconciler. Add a small `CompositeReconciler`
(hub `runtime/`) holding both and set **it** once; `reconcile_host(host)` calls the action service first, then the
package service, sequentially, ignoring neither's failure. Reconciliation procedure for a host: wake the worker (so
`queued` targets for the now-known host proceed) and, for each `dispatched` target of that host, ask
`JournalStatus::status_batch(host, [execution.as_action()], expected_epoch = target.dispatched_epoch)` and map the
answer exactly as prompt 21 maps actions:

| Journal answer | Target becomes |
|---|---|
| `Finished { succeeded: true }` | `Succeeded`, `outputs_confirmed = false`, audit `package.install_confirmed_by_journal`; the UI shows "refresh to confirm" |
| `Finished { succeeded: false, detail }` | `Failed`, code parsed from `detail` (`"<code>: ..."`) else `agent_refused` |
| `Running` | stays `Dispatched`; asked again at the next wake |
| `NotFound` | `NotRun{ code: "not_run" }`, "the agent has no record of this run; nothing was started" |
| `Unknown { NoStartRecord | AgentRestarted | JournalLost }` | `OutcomeUnknown` with the matching reason text (reuse 21's sentences) |
| epoch differs / unknown at dispatch | `OutcomeUnknown{epoch_changed | epoch_unknown}` |

### 6.8 Hub `[packages]` config (`crates/domovoi-config/src/hub/packages.rs`, **fixed** keys)

```toml
[packages]
enabled = false                    # default false; off = every /packages write endpoint answers 409 packages_disabled
max_artifact_bytes = 20971520      # 1 KiB .. 256 MiB; default 20 MiB (the agent's default compressed limit)
max_store_bytes = 536870912        # total artifact bytes kept; 1 MiB .. 16 GiB; default 512 MiB
max_artifacts = 64                 # 1 .. 1000
approval_window = "10m"            # how long an approved target may wait for its host; 1 min .. 1 h
job_history_days = 180             # terminal jobs older than this are purged; 7 .. 3650
revoked = ["e5f6a7b8c9d0e1f2"]     # hub-side revocation list (16 lowercase hex), file-only
[[packages.owner_keys]]            # file-only; the same table shape as the agent's trusted_keys (41's TrustedKey)
id = "a1b2c3d4e5f60718"
public_key = "<base64 of 32 bytes>"
label = "Owner"
```

`HubConfig.packages: PackagesHubSection` (`#[serde(default)]`, `deny_unknown_fields`, `JsonSchema`; **not** an
`Option`: the section exists with defaults, so absent means disabled). Validation (errors): bounds above,
`owner_keys` ids unique and 16 lowercase hex (reuse 41's shape check), `revoked` entries 16 lowercase hex, label
rules; warnings: `enabled = true` with no `owner_keys` ("no package can be imported"), a key both listed and
revoked (revoked wins). Hub start decodes every key (`ed25519_dalek::VerifyingKey::from_bytes`, base64 length, id
equals `KeyId::of_public_key`); an invalid key is a startup error naming the entry (same as 41's agent rule).
**File-only:** `owner_keys` and `revoked` must not be writable from the web settings API. Read
`crates/domovoi-hub/src/settings/patch.rs` and `stored.rs` for how paths are classified; if no "file-only"
mechanism exists, add the smallest one (a path list checked before any change is applied, answering the existing
"field set in hub.toml" 409 shape) and test it. `packages.enabled` and `packages.approval_window` **are** web
settings but need the password: add `StepUpRule { path: PathRule::Under("packages"), why: "package installation can
put code on hosts" }` to `STEP_UP_SETTINGS`, and update `STEP_UP_ACTIONS` so `installation.start` is documented as
enforced by the package job endpoints (keep the list, change its doc comment and `applies_once` text).

### 6.9 HTTP API (`/api/v1`, **fixed** paths, verbs, fields; **convention** DTO names)

All routes need `Authed` (session + CSRF as the other writes). Tag `packages`. Operation ids are snake_case
(`import_package`, `list_packages`, ...). Error bodies are `ErrorBody`; add stable `error` codes
`packages_disabled`, `package_conflict`, `package_in_use`, `store_full`, `unknown_key`, `revoked_key`,
`bad_signature`, `digest_mismatch`, `signature_file_invalid`, `not_zstd`, `too_large`, `plan_changed`,
`host_busy`, `no_such_package`, `no_such_job` (map them in `ApiError`; extend, do not restructure it).

| Method and path | Request | Success | Notes |
|---|---|---|---|
| `GET /packages/status` | none | `PackagesStatusDto { enabled, max_artifact_bytes, max_store_bytes, used_store_bytes, artifact_count, owner_keys: [{id, label}], revoked: [id], approval_window_secs }` | read; works when disabled (the wizard shows why it is off) |
| `GET /packages` | none | `[PackageDto]` | read; works when disabled |
| `POST /packages/import` | `multipart/form-data`: `archive` (file), `signature` (file) | `201 PackageDto` (`200` with `already_imported: true` when the same digest is already stored) | login + CSRF, no password (nothing runs). 409 `packages_disabled` when `enabled = false` |
| `GET /packages/{name}/{version}` | none | `PackageDetailDto` (the `PackageDto` plus `signature_toml`, `hosts: [{host_id, host_name, status, observed_at}]`, `file_status: ready | missing`) | |
| `DELETE /packages/{name}/{version}` | none | `204` | `409 package_in_use` while a non-terminal target references it; installed copies on hosts are untouched |
| `POST /packages/jobs/plan` | `PlanJobRequest { kind: "install"|"uninstall", name, version, host_ids: [uuid] }` | `InstallPlanDto { kind, name, version, archive_sha256?, key_id?, signer_label?, job_plan_hash, targets: [{ host_id, host_name, plan_hash, availability, installed: HostPackageDto?, warnings: [string] }] }` | read-only; install needs the artifact in the index; uninstall does not |
| `POST /packages/jobs` | `CreateJobRequest { kind, name, version, host_ids, plan_hash, password, confirm_text }` | `202 InstallJobDto` | step-up (6.9.1) |
| `GET /packages/jobs` | query `host_id?`, `name?`, `status?` (`active|finished`), `limit?`, `cursor?` | `JobPageDto { items: [InstallJobDto], next_cursor }` | newest first |
| `GET /packages/jobs/{id}` | none | `InstallJobDto` | |
| `POST /packages/jobs/{id}/targets/{host_id}/retry` | `RetryRequest { plan_hash, password, confirm_text }` | `202 InstallJobDto` | new attempt, new execution id, new authorization; only from `failed`, `not_run` or `outcome_unknown` |
| `POST /packages/jobs/{id}/targets/{host_id}/cancel` | none | `InstallJobDto` | only `queued` targets; any other state is `409 package_conflict` |
| `GET /hosts/{name}/packages` | none | `HostPackagesDto { host_id, observed_at?, entries: [HostPackageDto], install_allowed? }` | from `host_packages` (display only) |
| `POST /hosts/{name}/packages/refresh` | none | `HostPackagesDto` | login + CSRF; sends `agent.package.list`; `504`-style `unavailable` mapped through `ApiError` when the host is not connected |

`InstallJobDto { id, kind, name, version, archive_sha256?, key_id?, requested_by, created_at, status: running |
succeeded | failed | partial, job_plan_hash, targets: [InstallTargetDto] }` where the job `status` is **derived** at
read time (never stored): any non-terminal target -> `running`; all `succeeded` (cancelled targets ignored) ->
`succeeded`; none succeeded -> `failed`; otherwise `partial`. `InstallTargetDto { id, host_id, host_name, attempt, plan_hash,
status, failure: {code, message}?, outputs_confirmed, queued_at, started_at?, finished_at?, expires_at,
privilege_boundary? }`. Hash and digest fields are 64 lowercase hex; `key_id` 16 hex; times RFC 3339. The plan
and job DTOs never contain a secret, a path on the hub's disk or a package's contents.

**6.9.1 Step-up for a job (fixed).** In this order: session and CSRF (`Authed`); `packages.enabled`; request shape
(1 to 16 distinct hosts, name/version grammar from 41); `Auth::confirm_password(ip, user, password)` (wrong ->
403, lockout -> 429, exactly as settings); `confirm_text` must equal the package name (typed confirmation; else
403); recompute the plan and compare `plan_hash` (409 `plan_changed`); then `web_package_authorization(...)`
(new, in `crates/domovoi-hub/src/auth/approval.rs`, the only place a package approval is made: it builds
`HumanApproval::new(user, ApprovalChannel::Web { typed_confirmation: true }, now)` and
`ApprovalProvenance::new(Some(hex(session_hash)), "web")`). There is **no chat path** and no route accepts an
approval produced elsewhere. The approval window is `config.packages.approval_window`.

**6.9.2 Upload limits and streaming (fixed).** The route lives on a merged sub-router with
`DefaultBodyLimit::max(max_artifact_bytes + 64 KiB)` (read from the config when the router is built; a change needs a
restart; hard compile-time ceiling 256 MiB + 64 KiB). The handler reads fields with `field.chunk()`; it never calls
`field.bytes()` or `text()` on the archive. Exactly two file fields named `archive` and `signature` are accepted
once each; any other file field, a repeated field, or an unnamed field is `400`. The signature field is read into
memory but refused above `MAX_SIGNATURE_FILE_BYTES` (16 KiB). The archive is streamed into
`<data_dir>/packages/.tmp/<uuid>.part` (created `create_new`, mode 0600, directory 0700), hashing and counting as it
goes; the first byte past `max_artifact_bytes` aborts with `413 too_large` and deletes the part file.

### 6.10 CLI (**fixed** behaviour, **convention** flags)

`domovoi package import <archive> <sig> [--config PATH]` and `domovoi package list [--config PATH]`, in
`crates/domovoi/src/cmd/package.rs`, using `OfflineAdmin::open` (so they refuse to run beside a running hub, like
`audit verify`); `import` runs the same pipeline as the endpoint (`domovoi_hub::packages::import`) with
`Actor::System("cli")`, prints the package, version, digest, key id and label, and exits non-zero with the same
reason codes on refusal; works only when `packages.enabled = true`. There is **no** install command.

## 7. Ordered implementation steps

1. **Inspect and verify the repository context; report discrepancies.** Open every file and symbol in section 3.
   Confirm 24, 25 and 41 are merged and read their real names (`PackageName`, `PackageFailureCode`, `keys::*`,
   `package_contracts`, `ContractIndex`, `CapabilityRegistry::availability`, `PackageFixture`). Confirm that
   `AgentLink` still has no transfer or capability method, that `Registry::dispatch` still uses `try_send`, that
   `set_reconciler` keeps only the first reconciler, and that `TargetKey::parse` accepts `package:<name>` and
   `packages:all`. List `crates/domovoi-store/migrations/` and `docs/adr/` for the next numbers, and check whether
   26 or 34 added `AggregateKind`/`JournalKind`/`StateWrite` variants. Write every difference down before editing;
   stop if one changes the design.
2. **Core types** (6.1, 6.2): `install/*`, `PackagePlan`, `job_plan_hash`, `AggregateKind`/`JournalKind`/
   `StateWrite` variants with `as_str`/`parse`, the `InstallRepo` port, `TransferFrame`/`PackageCall`/`PackageDispatch`
   and the three `AgentLink` default methods, `PackageRelay`/`PackageCaps`. Fix every exhaustive `match`. Unit tests.
3. **Config** (6.8): `hub/packages.rs`, validation, schema snapshot, step-up rule, file-only paths.
4. **Migration and store** (section 8): tables, `InstallRepo`, the `StateWrite::Install` applier with its guards,
   `purge_install_jobs`, in-memory twin. SQLite tests.
5. **Hub package store and import pipeline** (6.5, 6.9.2): `keys.rs`, `verify.rs`, `store.rs`, `import.rs`; unit
   tests with `PackageFixture`.
6. **Link and registry** (6.3, 6.4): `Registry::dispatch_reserved`, `WsAgentLink::{send_transfer, start_capability,
   capability_result}`, `PolicyLink` forwarding, `runtime/caps.rs` `PackageCaps`, `FakeAgentLink` support. Link tests.
7. **Engine service**: `plan`, `create_job`, `worker`, `execute_package`, `settle`, inventory, `reconcile`
   (6.5, 6.7); wire the composite reconciler. Engine tests with the fake clock and fake link.
8. **Arch rules and ADR**: new `RAW_OPS` and boundary cards, the ADR; run `cargo xtask arch-check` and with `--base`.
9. **HTTP API and CLI** (6.9, 6.10): endpoints, DTOs, `ApiError` codes, `web_package_authorization`, route
   registration, OpenAPI regeneration, the `package` subcommand. Hub integration tests.
10. **Retention and scheduling**: register `purge_install_jobs` in the existing nightly maintenance; the worker
    start/stop in `runtime`. Crash tests.
11. **Docs** (section 15), then the checks and the completion report.

## 8. Database and persistence

**Migration.** `crates/domovoi-store/migrations/00NN_install_jobs.sql`, `NN` = the next unused number after listing
the directory on your branch (the last is `0011_plan_binding.sql` at the time of writing, so probably `0012`, unless
prompt 34 merged first). Never hard-code it; describe the migration by its contents. STRICT tables, integer
millisecond timestamps, hex checks in the style of `0011_plan_binding.sql`. **Enums (`status`, `kind`, `source`,
`observed_by`, `key_state`, inventory `status`) are validated in Rust, never by a SQL `CHECK`**; the only `CHECK`s
are hex lengths, positive sizes and `authorization_kind = 'human'`.

```sql
-- Imported packages: the index of the hub's package store. The bytes live on disk, content addressed.
CREATE TABLE package_artifacts (
    id             TEXT PRIMARY KEY,
    name           TEXT NOT NULL,
    version        TEXT NOT NULL,
    archive_sha256 TEXT NOT NULL CHECK (length(archive_sha256) = 64 AND archive_sha256 NOT GLOB '*[^0-9a-f]*'),
    key_id         TEXT NOT NULL CHECK (length(key_id) = 16 AND key_id NOT GLOB '*[^0-9a-f]*'),
    archive_size   INTEGER NOT NULL CHECK (archive_size > 0),
    signature_toml TEXT NOT NULL,
    signer_label   TEXT,
    source         TEXT NOT NULL,
    imported_by    TEXT NOT NULL,
    imported_at    INTEGER NOT NULL,
    UNIQUE (name, version)
) STRICT;
CREATE INDEX package_artifacts_digest ON package_artifacts (archive_sha256);

-- One owner request: install or remove one package version on a set of hosts.
CREATE TABLE install_jobs (
    id             TEXT PRIMARY KEY,
    kind           TEXT NOT NULL,
    name           TEXT NOT NULL,
    version        TEXT NOT NULL,
    archive_sha256 TEXT CHECK (archive_sha256 IS NULL OR (length(archive_sha256) = 64 AND archive_sha256 NOT GLOB '*[^0-9a-f]*')),
    key_id         TEXT CHECK (key_id IS NULL OR length(key_id) = 16),
    requested_by   TEXT NOT NULL,
    created_at     INTEGER NOT NULL,
    job_plan_hash  TEXT NOT NULL CHECK (length(job_plan_hash) = 64 AND job_plan_hash NOT GLOB '*[^0-9a-f]*')
) STRICT;
CREATE INDEX install_jobs_created ON install_jobs (created_at DESC);

-- One attempt on one host. A retry is a new row with the next attempt number.
CREATE TABLE install_job_targets (
    id                   TEXT PRIMARY KEY,
    job_id               TEXT NOT NULL REFERENCES install_jobs (id) ON DELETE CASCADE,
    host_id              TEXT NOT NULL,
    attempt              INTEGER NOT NULL CHECK (attempt >= 1),
    execution_id         TEXT NOT NULL UNIQUE,
    status               TEXT NOT NULL,
    plan_hash            TEXT NOT NULL CHECK (length(plan_hash) = 64 AND plan_hash NOT GLOB '*[^0-9a-f]*'),
    approved_by          TEXT NOT NULL,
    approval_channel     TEXT NOT NULL,
    approved_at          INTEGER NOT NULL,
    approval_session_ref TEXT,
    approval_client      TEXT NOT NULL,
    authorization_kind   TEXT NOT NULL CHECK (authorization_kind = 'human'),
    expires_at           INTEGER NOT NULL,
    artifact_transfer    TEXT,
    signature_transfer   TEXT,
    dispatched_session   TEXT,
    dispatched_epoch     TEXT,
    failure_code         TEXT,
    failure_message      TEXT,
    outputs              TEXT,
    outputs_confirmed    INTEGER NOT NULL DEFAULT 0,
    queued_at            INTEGER NOT NULL,
    started_at           INTEGER,
    finished_at          INTEGER,
    version              INTEGER NOT NULL DEFAULT 1,
    UNIQUE (job_id, host_id, attempt)
) STRICT;
CREATE INDEX install_targets_job    ON install_job_targets (job_id);
CREATE INDEX install_targets_status ON install_job_targets (status, queued_at);
-- At most one non-terminal target per host: the store enforces "one job per host at a time".
CREATE UNIQUE INDEX install_targets_one_active_per_host ON install_job_targets (host_id)
    WHERE status IN ('queued', 'transferring', 'dispatched');

-- What the hub last learned is installed on each host. Display only; never a decision input.
CREATE TABLE host_packages (
    host_id            TEXT NOT NULL,
    name               TEXT NOT NULL,
    version            TEXT NOT NULL,
    key_id             TEXT NOT NULL,
    archive_sha256     TEXT NOT NULL,
    installed_at       INTEGER NOT NULL,
    status             TEXT NOT NULL,
    key_state          TEXT NOT NULL,
    skills             TEXT NOT NULL,
    privilege_boundary TEXT NOT NULL,
    observed_at        INTEGER NOT NULL,
    observed_by        TEXT NOT NULL,
    PRIMARY KEY (host_id, name)
) STRICT;
```

**Artifact storage and limits.** `<hub.data_dir>/packages/` (0700): `<aa>/<archive_sha256>.tar.zst` (`aa` = first two
hex characters; files 0600), written to `.tmp/<uuid>.part`, `sync_all`, `rename`, directory sync. The signature text
(at most 16 KiB) is in the database, so a database backup restores the index and signature texts but **not the
artifact files**; a package whose file is gone shows `file_status: missing`, install settles `not_run{artifact_missing}`,
and re-importing the pair repairs it (say so in the docs). Caps: `packages.max_artifact_bytes` per archive,
`max_store_bytes` and `max_artifacts` for the store (beyond either: `409 store_full`). A file is deleted only when no
remaining row has the same digest. A startup sweep removes `.tmp/*` and files no row names, logging each.

**Import ordering (crash safety).** (1) stream and verify into `.tmp`; (2) rename into place; (3) commit
`StateWrite::Install(ImportArtifact)` with audit `package.imported`. A crash after (2) leaves an orphan the sweep
removes; there is never a row without a file. The same digest again is a no-op (`200`, `already_imported`); the same
`(name, version)` with another digest is `409 package_conflict` (delete the old one first; deleting an artifact
never touches installed copies).

**Transactions.** Every target state change, with its journal and audit rows, is one `Transaction`. `create_job` is
one transaction (job, all targets, audit per target); the partial unique index makes a second job for a busy host fail
the whole transaction (`CommitError::Constraint`, mapped to `409 host_busy`). Every `Target` write names the expected
`status` and `version`; a mismatch is `Conflict` and the caller re-reads. Nothing is written while frames are sent.

**Retention.** `Store::purge_install_jobs(now, cutoff)` deletes a job only when **all** its targets are terminal and
older than `packages.job_history_days` (`ON DELETE CASCADE` removes targets); it is called from the existing
`DailyJob::Retention` in `crates/domovoi-hub/src/runtime/schedulers.rs` right after `store.purge`. Artifacts and
`host_packages` are never auto-pruned. **Audit rows are never pruned** (RW-27); journal rows follow existing journal
retention.

**N-1 upgrade test.** The migration only adds tables, so the previous release ignores them. Add a test beside the
others in `crates/domovoi-store/tests/sqlite/upgrade.rs` (fixture `tests/fixtures/n1-schema5.db`): every existing row
survives, the four tables exist and are empty, and the partial unique index rejects two non-terminal targets for one
host.

## 9. Protocol and compatibility

- **Wire 1.2 is unchanged**: no new message, field, code or version; `PROTOCOL_VERSION` stays 1.2. This prompt only
  sends existing frames in 41's order and receives the existing `transfer_result`, `capability_result`, `refused`
  and `action_status_answers`.
- **Chunking.** 64 KiB raw per `transfer_chunk` (about 87 KiB encoded, far below `MAX_FRAME_BYTES`). The hub paces by
  outgoing-queue capacity (6.4) because chunks are not acknowledged; the agent applies backpressure through its own
  bounded queue.
- **Older or unconfigured agents.** An agent without prompt 41 never advertises the package contracts: the registry
  says `NotInstalled` and the target settles `not_run{not_advertised}` **before any transfer**. An agent without
  `[packages]` or with `allow_install = false` advertises at most `agent.package.list`: install and uninstall settle
  `not_run{not_advertised}` ("the agent's agent.toml has not enabled package installation"). The hub never tries to
  enable it.
- **Missing replies** are never success (6.3). **Hub downgrade**: tables only, an older hub ignores them.
- **API**: new endpoints only; the OpenAPI document and `web/src/api/schema.d.ts` change (section 15).

## 10. Error handling and recovery

**Request level** (nothing written):

| Situation | Behaviour |
|---|---|
| `packages.enabled = false`, a write endpoint | `409 packages_disabled`; reads still answer |
| import: not exactly the two named file fields, or a repeated, extra or unnamed file field | `400` |
| import: archive over `max_artifact_bytes` | `413 too_large`, part file deleted |
| import: signature file over 16 KiB, unparsable, `format != 1`, algorithm not `ed25519`, unknown field | `422 signature_file_invalid` |
| import: archive does not start with the zstd magic `28 B5 2F FD` | `422 not_zstd` |
| import: signed `key_id` not in `owner_keys` | `422 unknown_key` (names the key id, never the list) |
| import: key id in `revoked`, even when also in `owner_keys` | `422 revoked_key` |
| import: signature does not verify (`verify_strict` over `signing_message(..)`) | `422 bad_signature` |
| import: streamed SHA-256 differs from `archive_sha256` in the signature file | `422 digest_mismatch` |
| import: store would exceed `max_store_bytes` or `max_artifacts` | `409 store_full` |
| import: same name and version, other digest | `409 package_conflict` |
| plan/create: unknown or revoked host, duplicates, more than 16 hosts | `400`/`404` as `hosts::detail` does |
| create (install): artifact not in the index | `404 no_such_package` |
| create/retry: wrong or missing password, or typed text not equal to the package name | `403` (wrong password counts toward the login lockout; lockout `429`) |
| create/retry: plan hash differs from the recomputed one | `409 plan_changed` |
| create: a host already has a non-terminal target | `409 host_busy` naming the host; nothing created |
| retry: target not `failed`/`not_run`/`outcome_unknown`, or not the latest attempt | `409 package_conflict` |
| cancel: target not `queued` | `409 package_conflict` ("already started; it cannot be cancelled") |
| delete artifact referenced by a non-terminal target | `409 package_in_use` |
| refresh: host not connected | the existing not-connected mapping used by `hosts::logs` |

**Worker level** (each row settles the target and writes audit):

| Situation | Behaviour |
|---|---|
| host `Unknown`/`Absent` | stays `queued` until `expires_at`, then `not_run{approval_expired}` |
| host `Incompatible` | `not_run{agent_incompatible}` |
| contract not `Available` | `not_run{not_advertised}` with the registry's reason text |
| approval expired before `dispatched` | `not_run{approval_expired}`; a new request needs a new approval |
| artifact missing, or its digest differs from the index | `not_run{artifact_missing|artifact_corrupt}`; no `End` frame sent |
| `transfer_result` is `Failed` | `not_run{transfer_failed}` with the agent's code and detail |
| no `transfer_result` before the deadline, or the session ended | `not_run{transfer_failed}` ("no answer"); the next attempt uses new transfer ids |
| session or journal epoch changed since the snapshot | `not_run{session_changed}`; nothing is sent to a replacement |
| `hand_off` fails | `not_run{handoff_failed}` (proof nothing was queued) |
| `capability_result` `Failed`/`Refused` | `failed{code}`, the agent's code verbatim; nothing is installed (the agent's work is atomic) |
| `capability_result` timeout, dropped session, `Indeterminate` | stays `dispatched`; reconciled on the agent's next `hello`; never recorded as a failure |
| success outputs do not match the job | `outcome_unknown{result_mismatch}` and audit `package.install_mismatch` |
| commit conflict on a target write | re-read; stop if the target is no longer in the expected state |
| store failure on a commit | the target keeps its stored state; logged at error; the next wake retries from it |

**Hub restart** (every window):

| Stored state | After restart |
|---|---|
| `queued` | resumes on the first worker pass, or `approval_expired` if past `expires_at` |
| `transferring` | `not_run{hub_restarted}` at worker start (the agent's backend was never asked; it drops half transfers by its idle timeout and wipes staging at its own restart); the owner retries |
| `dispatched` | reconciled on the host's next authenticated `hello` (6.7); until then it stays `dispatched` and the UI says "waiting for the agent" |
| terminal states | unchanged |
| import crashed between rename and commit | orphan file swept at start |

**Agent crash mid-install** (41's windows): the journal has `started` without `finished`, so the answer is
`Unknown{AgentRestarted}` and the target becomes `outcome_unknown`. The owner presses "check host" (refresh) to see
what is there, then may retry; the retry uses a new execution id and a new authorization and the agent answers
idempotently (`already_installed`) or installs cleanly. **The hub never retries automatically after dispatch.**

## 11. Security requirements

- **Process identity and trust.** The hub runs no process for packages (R3/R4 untouched), never opens the archive and
  never writes outside `<data_dir>/packages/`. Paths are built only from the artifact digest (64 hex) and fixed names,
  never from an upload's file name or any request string. The hub has no code path that changes an agent's key set:
  `PackageCall` is a closed enum with no key field (6.3), and a gate test pins that.
- **Authorization.** Install and uninstall are human-only. The `HumanApproval` is made by `web_package_authorization`
  in `crates/domovoi-hub/src/auth/` (the only allowed place) after session, CSRF, password and typed package name,
  and is bound to the displayed plan hash. It is stored on the target row and restored by the store;
  `execute_package` runs only a target whose stored authorization is a human approval and unexpired.
  `AuthorizationKind::Policy` is refused (`NotYetSupported`). No chat path, no model tool, no endpoint accepting an
  approval from the client, no background creator of jobs.
- **Step-up and CSRF.** Password and typed confirmation for create and retry (`confirm_password` shares the login
  lockout). Import, cancel, delete and refresh need only session and CSRF: nothing executes, and refresh is a read on
  the agent. Changing `packages.*` through the settings API needs the password (6.8).
- **Hub key list.** `verify_strict` over exactly 41's `signing_message(..)`; revoked wins over listed; read from
  `hub.toml` at start, never mutated at runtime, not writable through the settings API. The agent ignores it.
- **Validation.** Names and versions with 41's `PackageName`/`PackageVersion`; the signature file with 41's
  `SignatureFile` (`deny_unknown_fields`) and its 16 KiB cap; the digest recomputed over the streamed bytes;
  multipart limited by field count, names and size; error bodies never echo uploaded text beyond the validated name
  and version; agent failure details are truncated to 500 characters.
- **Fail closed.** Unknown key, parse failure, missing file, digest difference, unadvertised contract, unknown
  availability, expired approval, changed session, changed plan: refuse or settle `not_run`. A target is `Succeeded`
  only on an agent success whose outputs match, or a journal `Finished{succeeded: true}`.
- **Single boundary (ADR and `xtask`).** In `xtask/src/arch/rules.rs`: add `send_transfer`, `start_capability` and
  `capability_result` to `RAW_OPS`; add EXEC-2 `BoundaryCard`s counting each exactly once in
  `crates/domovoi-hub/src/agent_server/skill_policy.rs` and once in `crates/domovoi-hub/src/runtime/caps.rs` (copy the
  `start_skill` cards); leave the existing `run_capability` "no caller" card untouched; add `BOUNDARY_PAIRS` and cards
  for `AuthorizedPackageTarget::of` and the `execute_package` call, one site each,
  `crates/domovoi-engine/src/packages/worker.rs` (follow how `AuthorizedExecution::of` and `execute::execute` are
  carded). Do **not** add the common names `transfer`, `hand_off` or `await_result` as raw ops; the gate test below
  asserts `crates/domovoi-engine/src/packages/execute.rs` is the only engine file naming `PackageRelay`. Extend
  `LINK_NAMES_ALLOWED_FILES` only if a new file must name the raw link (it should not). Write the ADR
  (`docs/adr/00NN-package-relay-boundary.md`: context, the closed `PackageCall`, the capability object, why jobs are
  not operation steps, the plan-hash family, the hub key list as a convenience and not an authority, what the checker
  cannot see). Run `cargo xtask arch-check` and `cargo xtask arch-check --base origin/overhaul-investigation`.
- **Audit.** One audit row per security-relevant change, in the same transaction as the state change; actor
  `Actor::User(user)` for owner actions, `Actor::System("packages")` for worker steps, `Actor::System("cli")` for the
  CLI import. Kinds (**fixed**): `package.imported`, `package.import_rejected` (detail `code`; written only after
  authentication), `package.deleted`, `package.install_requested`, `package.uninstall_requested` (per target; detail:
  package, version, digest, key id, host, execution id, plan hash, approval channel), `package.relayed`,
  `package.install_dispatched`, `package.installed`, `package.uninstalled`, `package.rejected` (agent said no;
  `code`), `package.not_run` (`code`), `package.outcome_unknown` (`reason`), `package.install_confirmed_by_journal`,
  `package.install_mismatch`, `package.cancelled`, `package.inventory_refreshed`, `package.key_untrusted_detected`.
  Subject `package:<name>@<version>` (list and refresh: `host:<id>`). Details never hold archive contents or the
  signature value. 04 §11.2 also lists `enabled`/`disabled`: those belong to the enabling action, not here.
- **Secrets.** None stored. The password is never logged or kept. `signature_toml` is public data, not a secret.
- **Resource bounds.** Streamed upload with a hard cap; at most 16 hosts per job; at most 4 hosts relaying; one active
  target per host; 64 KiB chunks; per-transfer deadlines; store caps; the only buffers are one chunk and the signature
  file.

## 12. Testing requirements

Fakes (existing unless marked): `FakeClock` (`advance`, `set`), `FakeAgentLink` (extend: scripted `transfer_result`
per transfer id, scripted `capability_result`, ordered recording of every frame, session and epoch controls, a slow-`End`
gate), the in-memory repos and `uow_apply` twin (extend), `FixtureHostFs`. Package bytes come from 41's
`PackageFixture` (`.build()`: archive, `sig_toml`, `signing_key`, `key_id`, `public_key_b64`, digest; `.sign_with`,
`.tamper_digest`). Tests never sleep. Engine tests drive `InstallService` directly; hub tests drive the router
in-process like `crates/domovoi-hub/tests/hub/settings.rs`; SQLite tests live in `crates/domovoi-store/tests/sqlite/`.

**Core and config.** `package_plan_hash_changes_with_each_input` (name, version, digest, key id, host, host key and
contract each flip the hash), `package_plan_hash_ignores_execution_id`, `package_plan_domain_is_not_plan_v2`,
`job_plan_hash_is_order_independent_and_kind_bound`, `install_status_terminal_set_is_exact`,
`aggregate_and_journal_kinds_round_trip`, `package_call_has_no_key_argument` (builds the three calls and asserts the
args hold only the `ARG_*` keys), `hub_packages_defaults_and_bounds`, `hub_owner_key_id_must_match_public_key`,
`hub_packages_warns_when_enabled_without_keys`, `packages_keys_are_not_web_writable`,
`packages_enabled_change_requires_step_up`, and the schema snapshot.

**Import pipeline** (`crates/domovoi-hub/src/packages/`):

| Test | Setup, action, expected |
|---|---|
| `owner_import_requires_trusted_owner_key` | signed by a key not in `owner_keys` -> `unknown_key`, nothing on disk; listed -> stored |
| `import_refuses_revoked_key_even_when_listed` | id in both lists -> `revoked_key` |
| `import_rejects_bad_signature_and_digest_mismatch` | flipped signature byte; `.tamper_digest()` -> `bad_signature` / `digest_mismatch`; no file, no row |
| `import_rejects_unknown_fields_and_oversize_signature_file` | `signature_file_invalid` |
| `import_rejects_non_zstd_archive` | text bytes -> `not_zstd` |
| `import_streams_and_caps_the_archive` | cap lowered; one byte over -> `too_large`, `.tmp` empty |
| `import_is_idempotent_and_conflicts_on_other_digest` | same pair twice -> one row, `already_imported`; other bytes under the same name/version -> `package_conflict` |
| `import_respects_store_caps` | `max_artifacts = 1`; `max_store_bytes` -> `store_full` |
| `import_never_opens_the_archive` | valid zstd magic, garbage body, correct digest and signature -> imports |
| `startup_sweep_removes_orphans_and_tmp_files` | stray `.part` and an unindexed file removed; indexed files stay |
| `delete_refused_while_referenced_and_keeps_shared_file` | non-terminal target -> `package_in_use`; two rows sharing a digest keep the file |

**Link and registry** (real `WsAgentLink` over the test transport, like `agent_server/link_tests.rs`):
`transfer_streams_320_chunks_without_busy` (20 MiB through a 256-slot outbox: all frames in order, `seq` gap-free),
`transfer_frames_for_one_transfer_stay_ordered`, `transfer_end_waits_for_the_reply_to_that_frame`,
`transfer_aborts_on_session_change`, `transfer_end_times_out_with_the_fake_clock`,
`start_capability_fails_fast_when_the_session_changed`, `capability_result_maps_failed_refused_and_in_progress`,
`policy_link_forwards_package_operations`.

**Engine** (`crates/domovoi-engine/src/packages/tests/`):

| Test | Setup, action, expected |
|---|---|
| `install_runs_transfers_then_capability_in_order` | frames: begin A, chunks, end A, begin B (kind `package_manifest`), chunks, end B, then one `run_capability` with `package`, `version`, `artifact_transfer = A`, `signature_transfer = B`, `authorization_kind = human`; target `succeeded`; inventory row written |
| `dispatch_is_committed_before_the_request_is_sent` | a link hook asserts the store holds `dispatched` with session and epoch when `start_capability` runs |
| `no_capability_without_human_authorization` | a non-human stored authorization never reaches the relay; the fake link records nothing |
| `expired_approval_is_not_run` | clock past `expires_at` while the host is absent -> `not_run{approval_expired}`, no frame |
| `host_unknown_waits_then_runs_on_wake` | `Unknown` stays `queued`; `Known` plus wake runs it |
| `not_advertised_settles_before_any_transfer` | registry `NotInstalled` -> `not_run{not_advertised}`, zero frames |
| `transfer_failure_is_not_run_and_never_dispatches` | each `TransferFailureCode` on the artifact and on the signature; no `run_capability` |
| `missing_transfer_result_is_a_failed_transfer` | `End` times out on the fake clock -> `not_run{transfer_failed}` |
| `artifact_missing_or_corrupt_is_not_run_without_end_frame` | file removed / byte flipped -> codes as named; no `End` |
| `agent_refusal_codes_pass_through` | each `PackageFailureCode` in a scripted `Failed` message -> `failed{code}` verbatim; unknown text -> `agent_refused` |
| `handoff_failure_is_not_run` | `start_capability` -> `NotConnected` -> `not_run{handoff_failed}` |
| `result_mismatch_is_outcome_unknown` | another digest or key id in outputs -> `outcome_unknown{result_mismatch}` plus audit |
| `session_change_between_transfer_and_dispatch_is_not_run` | replace the session -> `not_run{session_changed}` |
| `one_active_target_per_host` | second job for the host -> `host_busy`, no rows |
| `retry_creates_a_new_attempt_with_new_execution_and_authorization` | old attempt untouched; same plan hash required |
| `plan_hash_mismatch_refuses_create` | host key re-enrolled between plan and create -> `PlanChanged` |
| `partial_install_reports_per_host` | three hosts: success, `failed{untrusted_key}`, `not_run{host_unavailable}`; job status `partial`; each target keeps its result |
| `cancel_only_queued_targets` | queued -> cancelled; transferring -> conflict |
| `uninstall_sends_no_transfers_and_deletes_inventory` | only `run_capability{agent.package.uninstall}`; `removed` true or false both succeed |
| `refresh_replaces_inventory_and_audits_untrusted_keys` | scripted `OUT_PACKAGES` with a `disabled_untrusted_key` entry -> rows replaced, one `package.key_untrusted_detected`, none on an identical second refresh |
| `install_never_enables` | after success nothing in settings or skills changes; outputs `enabled = "false"` |

**Recovery** (`crates/domovoi-engine/src/packages/tests/recovery.rs`, in the style of prompt 12's crash harness where
it fits): `restart_in_transferring_settles_not_run_hub_restarted`, `restart_in_queued_resumes`,
`restart_in_dispatched_waits_for_the_agent`, `reconcile_finished_ok_marks_succeeded_unconfirmed`,
`reconcile_finished_failed_parses_the_code`, `reconcile_not_found_is_not_run`,
`reconcile_unknown_reasons_map_to_outcome_unknown` (table over `NoStartRecord`, `AgentRestarted`, `JournalLost`),
`reconcile_with_changed_epoch_is_outcome_unknown`, `reconcile_running_stays_dispatched`,
`composite_reconciler_calls_actions_then_packages`, `failure_after_dispatch_commit_and_before_send_reconciles_to_not_run`.

**Store** (`crates/domovoi-store/tests/sqlite/install.rs`): `create_job_is_atomic_with_audit`,
`second_active_target_for_a_host_fails_the_whole_job`, `target_write_with_stale_version_conflicts`,
`artifact_conflict_on_name_version_other_digest`, `purge_keeps_jobs_with_active_targets`,
`purge_deletes_old_terminal_jobs_but_not_audit`, `inventory_replace_is_atomic`, plus the N-1 test of section 8.

**API** (`crates/domovoi-hub/tests/hub/packages.rs`): `import_requires_login_and_csrf`, `import_multipart_end_to_end`,
`import_rejects_second_file_field_and_unnamed_field`, `status_and_list_work_when_disabled_but_writes_do_not`,
`create_job_requires_password_and_typed_name`, `create_job_wrong_password_counts_toward_lockout`,
`create_job_with_stale_plan_hash_is_409`, `create_job_for_busy_host_is_409_and_creates_nothing`,
`job_status_is_derived_from_targets`, `retry_and_cancel_rules`, `host_packages_and_refresh`,
`no_endpoint_accepts_a_client_supplied_approval`, `openapi_lists_the_package_routes`.

**Gates** (extend `crates/domovoi-engine/tests/gates.rs` and `crates/domovoi-hub/tests/gates.rs`; never weaken):
`hub_cannot_add_trusted_key` (no key field in `PackageCall`/`TransferFrame`; no route, setting or command carries an
agent `trusted_keys`; `owner_keys` is file-only), `package_requests_need_human_authorization` (only
`auth/approval.rs` builds a package approval; `AuthorizedPackageTarget::of` has one caller), `single_package_relay`,
`packages_never_open_archives` (no `ruzstd`, `tar` or `flate2` in the hub, engine or core manifests), `jobs_never_come_from_the_model_or_chat`
(source scan: no package-job creation reachable from `triage/`, `chat/`, `auth/interactions/`), and the
`exec_violations` fixtures for the new cards.

**CLI** (`crates/domovoi/tests/package_cli.rs`): `package_import_refuses_while_a_hub_runs`,
`package_import_stores_and_lists`, `package_import_reports_unknown_key_with_nonzero_exit`, `package_list_when_empty`.

Run Linux-only tests in WSL as the repository rules describe, and say which ran where.

## 13. Acceptance criteria

1. Nothing under `crates/domovoi-proto/` or `crates/domovoi-agent/` changes; no golden frame changes.
2. One migration adds exactly the four tables; the N-1 upgrade test passes; no merged migration is edited.
3. Import stores only packages signed by a key in the hub's `owner_keys`, not revoked, with a valid signature and a
   matching streamed digest; every other case is refused with the section 10 code and leaves no file and no row.
4. The hub never parses an archive and has no zstd/tar dependency; the upload is streamed and capped; one byte over
   the cap is `413` with no leftover file.
5. `owner_keys` and `revoked` cannot be changed through any API; no code path sends a key to an agent.
6. A job needs session, CSRF, password, the typed package name and a plan hash equal to the recomputed one; a host
   key re-enrollment or replaced artifact between preview and submit gives `409 plan_changed`.
7. The relay sends the artifact transfer, then the signature-file transfer (kind `package_manifest`), each answered by
   one `transfer_result`, then `run_capability` with 41's four arguments, in that order; a 20 MiB artifact transfers
   without a "busy" failure.
8. `dispatched` is committed before the request is sent; any failure before it is `not_run` with no `run_capability`
   frame; a result timeout is never recorded as a failure.
9. Results are recorded per target; the derived job status is `running`, `succeeded`, `failed` or `partial`; a retry is
   a new attempt with a new execution id and a new authorization; agent failure codes pass through verbatim.
10. The store allows one non-terminal target per host; a second job for a busy host creates nothing.
11. After a hub restart every state settles or reconciles as section 10 says; a `dispatched` target is resolved from
    the agent's journal as prompt 21 resolves actions; nothing is retried automatically after dispatch.
12. Install and uninstall are human-only; a policy authorization is refused; no chat, model or background path creates a
    job; `install_never_enables` passes.
13. Every state change writes its audit row in the same transaction; the section 11 kinds exist; none carries archive
    contents or the signature value.
14. `cargo xtask arch-check` (and `--base`) pass with the new cards and the ADR; `execute_package` is the only caller
    of the relay and the worker the only caller of `AuthorizedPackageTarget::of`.
15. `STEP_UP_ACTIONS` documents `installation.start` as enforced; changing `packages.*` settings needs the password.
16. Terminal jobs older than `job_history_days` are purged without touching audit rows, artifacts or inventory.
17. `domovoi package import` and `list` work offline and refuse beside a running hub.
18. OpenAPI and `web/src/api/schema.d.ts` are regenerated and committed; all named tests exist and pass; non-test files
    are at most 500 lines; no new `#[allow]`, `unwrap`, `expect` or `panic!` in library code.

## 14. Migration and compatibility

Existing installs: nothing changes until `[packages] enabled = true`; the new tables stay empty and write endpoints
answer `packages_disabled`. Existing agents: unchanged; those without prompt 41 or without `[packages]` settle
`not_advertised`. Existing config: the hub `[packages]` section is optional with defaults. Backups: the database backup
holds the index and signature texts but not the artifact files (documented; a missing file is `file_status: missing`
and re-import repairs it). Hub downgrade: the older hub ignores the tables. Prompt 47 makes installed packages
executable and prompt 49 builds the UI on section 6.9; neither needs a change to what is specified here.

## 15. Documentation requirements

- `cargo xtask docs-config` regenerates `docs/book/src/reference/configuration.md`; review the hub `[packages]` rows
  and accept the config schema snapshot only after reading the diff.
- OpenAPI: `cargo xtask openapi && cd web && npm run gen:api`; commit `web/src/api/openapi.json` and
  `web/src/api/schema.d.ts` (never hand-edit them).
- `docs/book/src/reference/hub.md` (verify the page name): a "Packages" section: the package store and its location,
  import by upload and `domovoi package import`, the hub key list and why it is not a trust authority, limits,
  retention, what a backup does and does not contain.
- `docs/book/src/reference/protocol.md`: how the hub uses `transfer_*` and `run_capability` for packages (order,
  chunk size, deadlines), complementing 41's agent text.
- `docs/book/src/security.md`: extend 41's paragraph: the hub offers and relays, the agent decides; install needs a
  password and typed confirmation; install never enables.
- `docs/book/src/guide/packages.md` (new, add to `SUMMARY.md`): the owner workflow (sign, import, preview, install,
  check, uninstall), the states and failure codes in plain words, what "outcome unknown" means and what to do.
- `ARCHITECTURE.md`: the install service and job aggregate beside the action service, §12 packages, §14 hub
  `[packages]`, §15 invariants (the hub cannot add keys; install never enables; jobs are human-authorized and
  single-path); §4 only if a dependency edge changed (it should not).
- `docs/adr/00NN-package-relay-boundary.md` (section 11). Run `cargo xtask docs-links`.
- Commits and the PR must not mention prompts, the build plan or coding agents.

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

Task-specific: no new dependency is added anywhere (the hub already has `base64`, `ed25519-dalek`, `sha2` and
`toml`; use workspace entries only). Prompts 24, 25 and 41 must be merged; if their names differ from the ones used
here, use the merged names and report each difference. Run Linux-only tests in WSL (`wsl.exe -d Ubuntu --`,
`CARGO_TARGET_DIR=$HOME/domovoi-target`) and say which tests ran where.

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

Task-specific checks: crates edited are `domovoi-core`, `domovoi-config`, `domovoi-store`, `domovoi-engine`,
`domovoi-hub`, `domovoi`, `domovoi-testkit` and `xtask`, so run `cargo nextest run -E 'rdeps(domovoi-core)'` and
`-E 'rdeps(domovoi-config)'`, `cargo xtask arch-check` (and `--base origin/overhaul-investigation`),
`cargo xtask docs-config`, `cargo xtask docs-links`, and `cargo xtask openapi && cd web && npm run gen:api`; for
`web/` run only `npm run typecheck` (only generated files change). Commit as `Rostislav <raikov0411@gmail.com>`; the
PR title has one scope, for example `feat(hub): import signed packages and relay installs to agents`.

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
