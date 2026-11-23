# 47c — Helper safety attestation: encoding, helper key and state, hub verifier

| | |
|---|---|
| Branch | `helper-attestation` |
| Milestone | H1 |
| Depends on (must be merged) | **47a** (`helper` accounts, sockets and skeleton: the helper process, its socket framing, its start checks, `domovoi helper init`, the installer; the 47a brief exists and its names are used below (see the 47a hand-off section); if the merged code differs, use the merged names and report each difference), 20 (wire 1.2: `attest_safety`, `safety_attestation`, `hello.helper_identity`), 22 (`KeyFingerprint`, `Authorization.safety_identity`), 34 (`ManagementCoverage`), 42 and 49 (merged: hub package relay, the Capability Center and its helper placeholders), 08 and 09 (unit of work, audit chain). |
| Can run in parallel with | nothing. Series order: 47a, 47b, 47c, 47d, 45, 46 (`README.md`). 47b is order-flexible with this prompt, never concurrent, and adds no migration. 47d, 45 and 46 run **after** this prompt. This prompt adds the first migration of the series (expected `0020`; 45 expects `0021` and 46 `0022`; always re-read the directory). |
| Produces (shared contracts) | `domovoi_core::attest::*` (the `04b` encoder and strict parser, both domain hashes, signing bytes, `SafetyIdentity` with its canonical text, `BootId`, `UnverifiedReason`, `ExpectedSafety`, the gate and decision functions); the helper's `safety.attest` operation, key lifecycle commands (`rotate-key`, `recover`, `fingerprint`; `init` completed), state file and route observer; `AgentLink::attest_safety`; the hub's `helper_trust` module (nonce store, strict verifier, latest-outcome gate, high-water mark); tables `helper_keys`, `helper_attestation`, `helper_high_water`; `StateWrite::HelperTrust`; `AttestedCoverage` (the real `ManagementCoverage`); `/api/v1/hosts/{name}/helper*` endpoints and DTOs; the real Helper tab and helper-key page of the Capability Center; frozen golden vectors C1 to C4, R1 to R4 and the new vectors; arch rules `ATTEST-1` to `ATTEST-10` and their ADR. |
| Consumes | 47a's helper skeleton (socket server, op dispatch, start checks, `init` directory creation, `selftest` ST-15); wire 1.2 frames `attest_safety`, `safety_attestation`, `error`, `hello`, `agent_status` (their **types** change here, their kinds do not); `ProcessExecutor` and `Clock` ports; 34's `ManagementCoverage` and `CoverageEvidence`; the step-up service (`crate::services::StepUp`); ADR 0032 and `04b` (the contract, decisions final). |
| Launch gate | **47a merged. R-10 accepted by the owner** (index §6.4; ADR 0032 "What acceptance still needs"). **The owner confirms the E5 default of section 6.5.6** (where `hub_addr` and `link_peer` come from) or names another source; nothing else in this brief waits on it. |
| Suggested model | strong (byte-exact contracts and the trust boundary: every decision is made below, follow the contracts exactly). Mechanical parts, once the core module exists, may go to a mid-size model: the DTOs, the web views, fixtures, the proptest cases. |

## 47a hand-off (read first)

47a's brief (`47a-helper-accounts-and-skeleton.md`) fixes the names below. ADR 0030 open question A2 is decided there (one new crate `domovoi-helper`, edges `core` and `config`); this brief adds **no crate and no crate edge**. Before editing, open 47a's merged code and confirm each item; a difference in a name is reported, a difference in a mode or token is a stop:

1. **Where the helper code lives:** `<helper>` is `crates/domovoi-helper/src/`. Every "helper side" file in section 4 goes under `<helper>/attest/`.
2. **The op dispatch and framing:** `serve/dispatch.rs` answers the closed `HelperRequest` (`SafetyAttest { nonce: Nonce }` exists as a placeholder answered `unavailable`; this brief gives it behaviour), framed by `domovoi_core::helper::frame` (`MAX_FRAME_BYTES = 65_536`, renamed `MAX_REQUEST_BYTES` by 47d). The wire spelling of the nonce is 47a's `Nonce` (64 lowercase hex); this brief's hub-side `Nonce32` converts to and from it (47d also uses `Nonce`). The group gate is `HelperConfig::effective_groups()` and `HelperGroup::SafetyAttest` (`safety.attest`, already in the default `groups`, `04` §8.3).
3. **What 47a's `domovoi helper init` creates:** the directory (root, 0700) and `state.lock` (0600) only. **This brief completes `init`**: it adds `attest.key` (0600) and the `state` file (generation 0, zero hashes, D-45). 47a's start checks accept an absent key and state in its milestone (`helper_start_accepts_absent_key_and_state_in_this_milestone`); this brief tightens that, and extends the token `uninitialized` to "no key and no state". **Stop and report if** 47a uses other file names, other modes or other token names than the ones in section 6.5.
4. **47a's start checks** (`run_start_checks`: directory root-owned 0700, files root-owned 0600, regular, no symlink, on opened descriptors) and the token `helper_state_permissions`. The attest path calls the same check and returns the same token; do not write a second implementation. 47a ships `write_atomic_locked` and `StateLock`; the key, state and every later helper write use them.
5. **How the agent reaches the helper:** `domovoi_agent::helper_client::HelperClient` (one request per connection). The agent's relay of `attest_safety` (section 6.6) uses it.

The helper spawns `ip` here, through the injected `ProcessExecutor` only (the series rule is stated once, in 47a section 5). If any item above does not exist, 47a is not merged: stop, the order is wrong.

## Hub use-case layer (read first)

Every command (anything that writes) is a method on a service in `crate::services`, taking `&Caller`; the HTTP
handler only parses, calls and maps the result (ADR 0026, `MOD-1`..`MOD-4`, `CALLER-1`). The new hub module
`helper_trust` is **support code** behind `services::HelperTrust` and is added to `UNZONED_MODULES`
(`xtask/src/arch/rules.rs:515`); do not add a handle to `ApiDeps`. Read-only DTO assembly may stay in the handler.

## 1. Objective

Give the hub a **proof of what the helper enforces** before any unattended containment can exist, and give the
owner the screens to enrol the helper's key. Concretely:

1. **The encoding** (`04b`): a production encoder and strict parser for the line-based safety report, both
   domain-separated hashes, the signing-bytes builder and the `SafetyIdentity` value, in `domovoi-core`, with the
   eight vectors C1 to C4 and R1 to R4 and the escaping vectors frozen as fixtures.
2. **The helper side**: `safety.attest` (nonce in, signed report out), the key lifecycle (`init`, `rotate-key`,
   `recover`, `fingerprint`), the state file with `flock` and atomic writes, every fail-closed condition and its
   token, `config_hash` and `route_state_hash` computation, route observation through `ip -j`, and the minimal
   incomplete report.
3. **The agent relay**: the agent answers `attest_safety` by passing the nonce to the helper and the signed bytes
   back unchanged.
4. **The hub side**: key pinning with fingerprint enrolment and rotation, nonce issue and late consumption,
   freshness measured from nonce issue time, `verify_strict` over the received bytes, the latest-outcome gate with
   revocation, and the persisted high-water mark, written in the acceptance transaction.
5. **Wire and core type fixes**, folded into the unreleased wire 1.2: required `expected_safety` for `Contain`,
   `deny_unknown_fields`, a closed reason enum, a fixed hash encoding; core `UnverifiableReason` gains
   `PathUncovered` and `ConfirmationStale`.
6. **The real `ManagementCoverage`** (`AttestedCoverage`) replacing `UnavailableCoverage`, and **the real
   Capability Center views** (helper status and attestation, helper-key enrolment) replacing 49's placeholders.
7. **Every RW-37 test by name** (section 12.2 says which are owned here, which by 45 or 46 and why), the two tests
   the research harness excluded, and property tests for the parsers.

It does **not** build containment, permits, `unit.restart`, the Docker helper, the skill runner, accounts or the
installer (section 5).

## 2. Architectural context

**Why this exists (D-28, RW-37).** Unattended containment on a host needs owner-confirmed management paths, and
a path may be confirmed only when a **verified, fresh helper attestation** shows it covered. The hub never trusts
an agent's description of the helper. The helper (root) computes a report of the effective safety set and signs
it with an Ed25519 key only it can read; the agent relays the bytes unchanged; the hub pins the key after the
owner compares fingerprints and verifies every report against the pinned key.

**Decisions are final (owner, `synthesis 00 §5a`, D-44 to D-52; record in `docs/rework/99-open-decisions.md`).**
Do not reopen them; where a question looks open, the table below says where it was settled.

| Topic | Decision | Where |
|---|---|---|
| empty `incomplete_reason` | empty iff `complete=true` | D-44, `04b` §1 |
| incomplete report | one fixed minimal shape | D-44, `04b` §3 |
| `init` | generation 0, zero hashes, no observer; first real attestation is generation 1 | D-45 |
| selector ambiguity | interface name escaped inside the item; R4 and the other seven vectors unchanged | D-46, `04b` §2 |
| wire types | required `expected_safety` for `Contain`, `deny_unknown_fields`, closed reason enum, fixed hash encoding; **folded into unreleased 1.2, no 1.3** | D-47, 00 §5a (protocol row) |
| revocation | latest outcome; revoke on key mismatch, signed incomplete report, generation regression | D-48 |
| rollback | hub high-water mark per helper and key, in the acceptance transaction | D-49 |
| limits | refuse an over-long TTL; `max_active=0` means none | D-50 (applied by 45/46) |
| missing `expected_safety` | refuse | D-51 |
| grammars, timing, tokens | harness readings with the synthesis amendments; freshness from nonce issue time; nine wire tokens; retired private keys deleted; unrepresentable routing refused as incomplete | D-52 |
| permission-fault token | `helper_state_permissions` (settled in 00 §5a; ADR 0032 E2 still lists it open: update the ADR) | 00 §5a |

**Trust boundaries (INV in `ARCHITECTURE.md` §15 is extended here).** The helper is the only holder of the key.
The agent is untrusted for this purpose: it can withhold, delay or replay a relay, and can send a wrong key (a
denial of service, recovered by the next honest attestation under the latest-outcome rule). It cannot forge a
report, widen the helper's set or read the key. The hub's pin is the only trust root; the key id is a label.

**Crate boundary (decided from the rules; no new crate, no rule loosened except the additions of section 11).**

| Code | Crate | Why (cite) |
|---|---|---|
| encoder, strict parser, line types, hashes, `signing_bytes`, `SafetyIdentity`, `BootId`, `UnverifiedReason`, `ExpectedSafety`, gate and decision functions | `domovoi-core` `attest/` | pure, needs only `sha2`, `hex`, `serde`, `thiserror`, `jiff`, all in core's allowlist (`xtask/src/arch/rules.rs:19-33`); the helper process cannot name `domovoi-proto` (binary crate edges, `ARCHITECTURE.md` §4), so the guard value type lives in core and `domovoi-proto` re-exports it |
| TOML to typed `[safety]` values | `domovoi-config` | core has no `toml`; `toml` is in the config allowlist; core builds the canonical lines from the typed values, so the C1 to C4 tests span both crates |
| Ed25519 signing, key and state files, `flock`, route observer, `safety.attest` | `<helper>` (47a's choice) | I/O and the key; **no new crate**, a new crate would change `CRATES` and need an ADR |
| `verify_strict`, nonce store, pin handling, acceptance | `domovoi-hub` `helper_trust/` | the hub already has `ed25519-dalek` (`crates/domovoi-hub/Cargo.toml:35`); one module holds the only verify call (`ATTEST-1`) |
| attestation-backed coverage | `domovoi-engine` `knowledge/attested.rs` | pure decision over a core read port `HelperTrustRead`; engine does no I/O (R2) |
| tables | `domovoi-store` | `StateWrite::HelperTrust`, like `StateWrite::Install` |

Two traps in core: the arch scan forbids the literal text `std::net` (`rules.rs:574`, `PURE_FORBIDDEN_PATHS`), so
use `core::net::{IpAddr, Ipv4Addr, Ipv6Addr}` as `crates/domovoi-engine/src/correlate.rs:124` does; and core has
no `toml`, no `ed25519-dalek` and no `rand`. `flock` uses `std::fs::File::lock` (stable since Rust 1.89; the
workspace `rust-version` is 1.94), so no `nix` and no new dependency. The helper crate adds `ed25519-dalek` and
`rand` as `{ workspace = true }` (both are already workspace dependencies).

## 3. Repository context

Verified in this checkout (branch `research-synthesis`, HEAD `b8021381`, which is documentation on top of the
implementation tree). Re-open each file before editing and report differences. **Migrations end at
`0019_install_jobs.sql` on this checkout; that is an observation, not a plan: list
`crates/domovoi-store/migrations/` on the branch you cut and take the next unused number** (47c is the first migration of the series, so `0020` is expected if nothing else has merged; 45 expects the next number and 46 the one after; re-read the directory, never assume).

**Wire (`crates/domovoi-proto`).**

- `src/version.rs:17`: `PROTOCOL_VERSION` is already `1.2`. No bump is made (decided: 1.2 is unreleased; the
  latest release carries 1.1).
- `src/messages/actions.rs:34-36`: `RunCapability.expected_safety: Option<ExpectedSafety>` with `#[serde(default)]`.
  `:57-75`: `ExpectedSafety` is internally tagged by `state` with `Verified { fingerprint: String, boot_id:
  String, generation: u64, config_hash: Sha256Bytes, route_state_hash: Sha256Bytes }` and `Unverified { reason:
  String }`. **Differences from the contract:** no `deny_unknown_fields`, `fingerprint` and `boot_id` are bare
  strings, `reason` is free text.
- `src/messages/stream.rs:102-117`: `AttestSafety { nonce: Nonce }` and `SafetyAttestation { public_key:
  PublicKey, report: Base64Bytes, signature: Signature }`. `src/messages/handshake.rs:195-201, 247-272`:
  `hello.helper_status`, `hello.helper_identity: Option<HelperIdentity>`, `HelperStatus { Absent, Ok{ops},
  Error{reason} }`, `HelperIdentity { public_key: PublicKey }`; `stream.rs:85-88` repeats both on `AgentStatus`.
- `src/bytes.rs:121-134`: `Sha256Bytes` (32 bytes, **lowercase hex**, uppercase or wrong length refused at decode),
  `Base64Bytes` capped at `MAX_BASE64_BYTES` = 64 KiB at decode.
- `tests/protocol/extensions.rs:69-105`: `expected_safety_tagged_variants_roundtrip` and
  `malformed_expected_safety_refused` use `boot_id: "5f0c"`, `fingerprint: "ab".repeat(32)` and a free-text
  reason: they must be **rewritten** with valid values (they would fail under the strict types).
- `tests/protocol/snapshots.rs:40-45`: `golden_frames_stay_at_1_1` pins the literal files under
  `tests/protocol/fixtures/1_1/`: **never touch those files.** The 1.2 frames are `insta` snapshots in
  `tests/protocol/snapshots/` (`run_capability.snap` has `"expected_safety": null`). The 1.2 type changes of
  this brief do not change an existing snapshot (the field stays optional); two new samples add two snapshots
  (section 9). If an existing snapshot changes, a serialisation changed by accident: stop and find out why.

**Agent.** `crates/domovoi-agent/src/link/mod.rs:425-455`: the request dispatch has no `Message::AttestSafety`
arm; a frame of that kind falls to the `Unknown` handling and is answered `unsupported`. The agent announces
`helper_identity: None` (`link/handshake.rs:56-58, 125-126`). `Message::Error(ErrorMessage { code: String,
message: String })` exists (`messages/stream.rs:60-65`).

**Hub.**

- `crates/domovoi-hub/src/agent_server/live.rs:44` and `session.rs:49`: `Live` keeps `helper_status` but **drops
  `hello.helper_identity`**, and `Registry::set_agent_status` stores the last `AgentStatus` whole. The hub never
  reads a helper key today.
- `session.rs:227`: replies of kind `SafetyAttestation` (and `TransferResult`, `CapabilityResult`, `Refused`,
  `ActionStatusAnswers`, `Error`) are already delivered to whoever waits on `reply_to`. `AgentLink` has **no**
  attestation method (`ports/agent_link.rs`: `run_tool` ... `get_catalog`, `send_transfer`, `start_capability`,
  `capability_result`).
- `crates/domovoi-hub/src/settings/step_up.rs:103` (`STEP_UP_ENFORCED_ACTIONS`: `policy.publish`,
  `policy.lock_changed`) and `:118` (`STEP_UP_ACTIONS`: `installation.start` is enforced by the package job
  endpoints). `crate::services::StepUp` is the password check the package service uses
  (`services/packages/jobs.rs:18`).
- `crates/domovoi-hub/src/runtime/knowledge.rs:20` wires `UnavailableCoverage` into `KnowledgeService`;
  `crates/domovoi-hub/examples/dev_hub.rs:521` and `tests/hub/profile_backfill.rs:19` do the same.

**Core and engine.**

- `crates/domovoi-core/src/knowledge/paths.rs:64-100`: `UnverifiableReason` has eight variants (`NotAvailable`,
  `KeyNotPinned`, `KeyMismatch`, `AttestationMissing`, `AttestationStale`, `SignatureInvalid`,
  `ReportIncomplete`, `GenerationRegressed`) and `describe()`; it **lacks `PathUncovered` and
  `ConfirmationStale`**. `CoverageEvidence` (`:10-30`) is documented as "stored and compared for equality only,
  never parsed here"; the reboot exception (a confirmation survives a changed `boot_id`) needs a parse, so the
  doc comment changes (section 6.9).
- `crates/domovoi-core/src/ports/knowledge.rs:114-122`: `ManagementCoverage { is_available(); verdict(host, cidr)
  }`. `crates/domovoi-engine/src/knowledge/coverage.rs`: `UnavailableCoverage` (never `Covered`). Knowledge
  service use: `engine/src/knowledge/paths.rs:139-143` maps `Covered`/`Uncovered`/`Unverifiable`.
- `crates/domovoi-core/src/host.rs:127-137`: `KeyFingerprint` (`sha256_newtype!`, `of_public_key(&[u8; 32])`,
  64 lowercase hex). Reuse it for the helper key; do not add a second hashing implementation.
- `crates/domovoi-core/src/action/authorization.rs:117`: `safety_identity: Option<String>`: a free string today.
  Section 6.1 defines its canonical text; no field is added here.
- `crates/domovoi-core/src/ports/process.rs:74`: `ProcessExecutor`, `ProcessSpec { program, args, env, cwd,
  stdin, timeout, max_output_bytes, run_as }`: argv only, empty environment (R3, R4).

**Config.** `crates/domovoi-config/src/agent.rs:50`: `AgentConfig.hub_url: Url`. `docs/rework/04-target-architecture.md`
§8.3 gives the `helper.toml` shape; the installer writes it `root:root 0600`; `agent.toml` is `root:root 0644`
(synthesis 02 §5.7).

**Web.** `web/src/features/capabilities/`: `HelperTab.tsx`, `HelperPlaceholder.tsx`, `HelperKeyPage.tsx`,
`HelperKeyPlaceholder.tsx`, and the gate test `capabilityGates.test.ts:14` (`capability_center_has_no_helper_data_path`)
whose failure text says "replace the placeholder and update this test". Routes: `web/src/app/routes.tsx:85`
(`/capabilities/helper-key`).

**Research reference implementation** (read, port, do not import): `docs/rework/investigations/probes/r10/`
(standalone workspace, 95 tests passing on three hosts). Port map: `src/encoding.rs`, `lines.rs`, `hash.rs`,
`report.rs` to core `attest/`; `config.rs` to core + `domovoi-config`; `routes.rs` to the helper's `observe`;
`state.rs`, `keys.rs`, `helper.rs` to the helper; `hub.rs` to `helper_trust/`; `coverage.rs`, `decision.rs`,
`safety.rs` to core `attest/gate.rs`, `decide.rs`, `expected.rs`. The harness is **not** the contract where the
synthesis amends it; section 6 states the amended behaviour.

**Differences between planning text and repository** (list them in your report; none changes the design):

| # | Planning text | Repository |
|---|---|---|
| 1 | `04b`/RW-37: observation "via netlink" | decided here: `ip -j` through `ProcessExecutor` (same CLI choice as `nft`, D-56); netlink binding stays a later idea |
| 2 | synthesis 03 names the permission token `permissions_unsafe` | 00 §5a: `helper_state_permissions`; ADR 0032 still lists E2 open |
| 3 | synthesis 03 names `ATTEST-4` to `ATTEST-6`; ADR 0032 names `ATTEST-4` to `ATTEST-10` | use ADR 0032's numbering and meanings |
| 4 | synthesis 03 says report cap 60 KiB (pseudocode) and 64 KiB (5.9) | one cap: `MAX_BASE64_BYTES` (65 536 bytes), the decode cap of `Base64Bytes` |
| 5 | harness token `rule_unrepresentable` | `route_unrepresentable` (synthesis 03 E6, `04b` §3) |
| 6 | RW-37 "hub stores `helper_keys(host_id, public_key, fingerprint, pinned_at, pinned_by)`" | same columns, plus tables for the verified state and the high-water mark (section 8) |
| 7 | the hub is described as holding the received key | it does not today (`live.rs:44`); 47c stores it from `hello` and `agent_status` |
| 8 | wire `ExpectedSafety` in `domovoi-proto` | moves to `domovoi-core::attest` and is re-exported from `domovoi_proto::messages` (the helper cannot name proto); the path `domovoi_proto::messages::ExpectedSafety` stays valid |

## 4. Exact scope

Files to **add** (non-test files at most 500 lines, web files at most 250; split rather than exceed):

| File | Purpose |
|---|---|
| `crates/domovoi-core/src/attest/{mod,field,scalar,lines,selector,config,hash,report,parse,identity,boot,reason,expected,gate,decide}.rs` | the pure `04b` implementation (6.1 to 6.4, 6.7.4, 6.9) |
| `crates/domovoi-core/src/ports/helper_trust.rs` | `HelperTrustRead` read port (6.12) |
| `crates/domovoi-core/tests/attest/{mod,vectors,invalid,escaping,parser_props,identity,gate}.rs` and `tests/attest/vectors/*` | vector, strictness and property tests (12) |
| `crates/domovoi-config/src/helper_safety.rs` | strict `[safety]` TOML to raw typed values (6.5.1); if 47a already added a `helper.toml` loader, the `[safety]` section goes into **that** loader, not a second parser |
| `<helper>/attest/{mod,keys,state,lock,compute,observe,ip_source,ip_json,endpoints,attest,guard,lifecycle}.rs` | helper side (6.5) |
| `crates/domovoi-agent/src/requests/attest.rs` | agent relay of `attest_safety` (6.6) |
| `crates/domovoi-engine/src/knowledge/attested.rs` | `AttestedCoverage` (6.12) |
| `crates/domovoi-hub/src/helper_trust/{mod,nonce,verify,gate_view,request,accept,pin,schedule,detail}.rs` | the hub's trust module (6.7) |
| `crates/domovoi-hub/src/services/helper_trust.rs` | `services::HelperTrust`: pin, unpin, recheck, view (6.7.6) |
| `crates/domovoi-hub/src/api/{helper,dto_helper}.rs` | endpoints and DTOs (6.10) |
| `crates/domovoi-store/migrations/00NN_helper_trust.sql` | three tables (section 8) |
| `crates/domovoi-store/src/{helper_trust}/{mod,apply,codec}.rs`, `repos/helper_trust.rs` | `HelperTrustRead` over SQLite, the `StateWrite::HelperTrust` applier |
| `web/src/features/capabilities/{HelperPanel,HelperHostCard,HelperAttestation,HelperKeyEnroll,helperText}.tsx` (and `.ts`), `web/src/api/hooks/helper.ts`, `web/src/design/helperVocab.ts` | the real views (6.11) |
| `docs/adr/0035-helper-attestation-boundary.md` (expected number: 47a 0033, 47b 0034, then this one; take the next free number if different) | required because `xtask/src/arch/rules.rs` changes (11) |
| fixtures: `.gitattributes` entry marking the vector and fixture directories `-text` | LF is part of the vectors (12.4) |

Files to **change** (minimal edits): `crates/domovoi-core/src/{lib.rs,knowledge/paths.rs,knowledge/mod.rs,
ports/{agent_link,knowledge,mod}.rs,ports/repos/mod.rs,ports/uow/{transaction,journal,mod}.rs}`,
`crates/domovoi-proto/src/{lib.rs,messages/{actions,mod}.rs}` and its tests (`extensions.rs`, `samples.rs`,
snapshots), `crates/domovoi-agent/src/{link/mod.rs,requests/mod.rs,link/handshake.rs}`, `crates/domovoi-hub/src/{lib,
agent_server/{live,session,registry,link,skill_policy?,mod},runtime/{mod,knowledge,caps?},settings/step_up,api/mod}.rs`
and `examples/dev_hub.rs`, `crates/domovoi-store/src/{lib,uow/writes,purge}.rs` and `repos/mod.rs`,
`crates/domovoi-engine/src/knowledge/{mod,paths}.rs`, `crates/domovoi-testkit/src/{coverage,link,repos/*}.rs`,
`xtask/src/arch/rules.rs`, hub and core `tests/gates.rs`, `web/src/features/capabilities/{HelperTab,HelperKeyPage,
capabilityGates.test}.ts*`, `web/src/app/` (visibility ids), `web/DESIGN.md`, `web/src/api/{openapi.json,schema.d.ts}`
(generated only), the `<helper>` dispatch and `domovoi helper` subcommand tree of 47a, docs (section 15).

Files to **delete**: `web/src/features/capabilities/HelperPlaceholder.tsx`, `HelperKeyPlaceholder.tsx`.

## 5. Explicit non-goals

- **No containment**: no `firewall.block_source`, no nft, no permit issuance, no `PolicyPermit.safety_identity`
  field, no `Contain` request type, no new `RefusalCode` variants (`malformed_safety`, `safety_changed`,
  `safety_unknown` belong to 46, which spells them as its own brief decides with the code that raises them). 47c delivers the
  validating constructor and the identity comparison those use (6.7.4, 6.5.8).
- **No accounts, installer, unit files, sockets, `selftest`, start checks** (47a) and **no skill runner** (47b). 47c
  uses 47a's start check; it does not rewrite it.
- **No write groups** (`unit.restart` is 45), no Docker helper (D-24), no mutation limiter (the first helper write
  group, 45, owns `[limits] mutations_per_hour` persistence).
- **No hub-signed permit**, no owner-signed approvals (RW-37 "separate architectural change").
- **No netlink binding, no new crate, no new dependency** beyond `ed25519-dalek` and `rand` in the helper crate
  (workspace versions).
- **No hub write of `helper.toml`** and no API that edits `[safety]`, adds an exclusion or pins a key without the
  password and the typed fingerprint. No chat path, model tool or background task pins or unpins a key.
- **No protocol version bump** and no change to any file under `crates/domovoi-proto/tests/protocol/fixtures/1_1/`.
- **No skill-runner status in the web views** (47b); the existing `root_agent` and `runner_installed` labels of the
  Inventory tab stay.
- No UI for unverified human approval cards (46).
- Do not weaken any gate test or arch rule; do not touch merged migrations.

## 6. Required interfaces and contracts

Marked **fixed** (must match) or **convention** (follow repository style). Everything the synthesis left to the
owner is decided in `04b` and ADR 0032; this section restates what you build and cites where each rule was
settled.

### 6.1 Core: `domovoi_core::attest` (new, pure; **fixed** names and semantics)

```rust
// constants
pub const REPORT_FORMAT: u32 = 1;
pub const SIGNING_PREFIX: &str = "domovoi-safety-attest-v1\n";
pub const CONFIG_PREFIX: &str = "domovoi-safety-config-v1\n";
pub const ROUTES_PREFIX: &str = "domovoi-safety-routes-v1\n";
/// Equal to domovoi_proto::MAX_BASE64_BYTES (a proto test asserts the equality).
pub const MAX_REPORT_BYTES: usize = 64 * 1024;
pub const NONCE_TTL: SignedDuration = SignedDuration::from_secs(60);        // strict: age >= 60 s is expired
pub const FRESHNESS: SignedDuration = SignedDuration::from_secs(300);       // strict: age >= 300 s is stale
pub const CONFIRMATION_MAX_AGE: SignedDuration = SignedDuration::from_secs(30 * 86_400);
pub const MANDATORY_UNITS: [&str; 4] = [
    "domovoi-agent.service", "domovoi-helper.socket", "domovoi-skill-runner.socket", "sshd.service",
];
pub const ZERO_HASH: Hash32 /* 32 zero bytes */;

/// Strict 32-byte hash: exactly 64 LOWERCASE hex characters on the wire. Core cannot name proto's `Sha256Bytes`,
/// and core's `sha256_newtype!` (`crates/domovoi-core/src/sha256.rs`) serialises lowercase but its decoder uses
/// `hex::decode_to_slice`, which ACCEPTS uppercase; so `Hash32` is a new core type with a strict decoder
/// (refuses uppercase and wrong length). `fingerprint` fields use core's `KeyFingerprint` where the value is a
/// key fingerprint, with the same strictness added by a check before parsing. Proto's `Sha256Bytes` stays for
/// its other fields; a test proves `Hash32` and `Sha256Bytes` serialise identically.
pub struct Hash32([u8; 32]);

// field codec (04b section 1; strict percent-encoding)
pub fn is_literal(byte: u8) -> bool;                          // A-Z a-z 0-9 . _ : / @ -
pub fn encode_field(bytes: &[u8]) -> String;                  // uppercase %XX for everything else
pub fn decode_field(text: &str) -> Result<Vec<u8>, EncodeError>; // refuses lowercase hex, an encoded literal, a truncated escape, a raw byte outside literal+'%'

// typed scalars (scalar.rs): canonical decimal u32/u64/port, Family, Cidr (network and ifaddr forms), ip (RFC 5952 lowercase, via core::net)
pub fn parse_uint(text: &str) -> Result<u64, EncodeError>;     // "0" or [1-9][0-9]*, no sign
pub fn parse_u32(text: &str) -> Result<u32, EncodeError>;
pub fn parse_port(text: &str) -> Result<u16, EncodeError>;     // 1..=65535
pub fn parse_ip(text: &str) -> Result<IpAddr, EncodeError>;    // refuses uppercase IPv6, leading-zero IPv4, non-compressed forms
pub struct Cidr { pub addr: IpAddr, pub prefix: u8 }
impl Cidr { pub fn parse_report(text: &str) -> Result<Self, EncodeError> /* host bits zero */;
            pub fn parse_ifaddr(text: &str) -> Result<Self, EncodeError> /* host bits kept */;
            pub fn parse_config(text: &str) -> Result<Self, EncodeError> /* normalises host bits; refuses no prefix */;
            pub fn network(&self) -> Self; pub fn subset_of(&self, outer: &Self) -> bool; pub fn overlaps(&self, other: &Self) -> bool;
            pub fn render(&self) -> String; }
pub fn check_ifname(bytes: &[u8]) -> Result<(), EncodeError>;  // 1..=15 decoded octets; kernel rules are not enforced on the wire

// line types, each: render() -> String (the value, already encoded) and parse(&str) -> Result<Self, EncodeError>
pub struct LinkPeer { pub ip: IpAddr, pub port: u16 }
pub struct LocalAddr { pub addr: Cidr, pub ifname: Vec<u8> }
pub struct Route { pub family: Family, pub table: u32, pub dst: Cidr, pub gateway: Option<IpAddr>, pub ifname: Vec<u8>, pub metric: u32 }
pub struct Rule { pub family: Family, pub priority: u32, pub selector: Selector, pub table: u32 }
pub struct EndpointRoute { pub endpoint: IpAddr, pub table: u32, pub gateway: Option<IpAddr>, pub ifname: Vec<u8>, pub src: IpAddr }
pub struct Selector { /* all | items in the fixed order from, to, iif, oif, fwmark, ipproto, sport, dport, uidrange, not */ }
//   iif and oif are Vec<u8> (bytes, never String); render() strict-encodes the name BEFORE joining items with ';'
//   (inner escaping, D-46), and the Rule line then encodes the whole selector once as a field.

// config lines (config.rs): typed input in, canonical lines and hash out. TOML parsing is NOT here.
pub struct SafetyInput { pub management_cidrs: Vec<String>, pub protected_units: Vec<String>,
                         pub protected_containers: Vec<String>, pub max_ttl: Option<String>, pub max_active: Option<u64> }
pub struct ConfigLines { /* cidr*, protected_unit*, protected_container*, max_ttl_s, max_active */ }
impl ConfigLines {
    pub fn effective(input: &SafetyInput) -> Result<Self, ConfigError>;  // normalise, canonicalise units (.service default), union MANDATORY_UNITS, dedupe, sort; max_ttl default 3600, max_active default 0
    pub fn lines(&self) -> Vec<String>;                                 // "key=value" in report order
    pub fn hash(&self) -> Hash32;                                  // domain_hash(CONFIG_PREFIX, lines)
}
pub fn parse_max_ttl(text: &str) -> Result<u64, ConfigError>;           // digits then one of s m h d, value >= 1, seconds fit u64
pub fn canonical_unit(name: &str) -> Result<String, ConfigError>;       // known type suffix, [A-Za-z0-9:_.@-]{1,255}, no backslash
pub fn check_container(name: &str) -> Result<(), ConfigError>;          // [A-Za-z0-9][A-Za-z0-9_.-]{0,254}

// hashes and signing (hash.rs)
pub fn domain_bytes(prefix: &str, lines: &[String]) -> Vec<u8>;         // prefix, then each line + LF
pub fn domain_hash(prefix: &str, lines: &[String]) -> Hash32;
pub fn signing_bytes(report: &[u8]) -> Vec<u8>;                         // SIGNING_PREFIX || report

// report (report.rs, parse.rs)
pub struct RouteLines { pub hub_addr: Vec<String>, pub link_peer: Vec<String>, pub local_addr: Vec<String>,
                        pub route: Vec<String>, pub rule: Vec<String>, pub endpoint_route: Vec<String> }
impl RouteLines { pub fn lines(&self) -> Vec<String>; pub fn hash(&self) -> Hash32; pub fn normalise(&mut self); }
pub enum Report { Complete(CompleteReport), Incomplete(IncompleteReport) }
pub struct CompleteReport { pub helper_key_id: String, pub nonce: Nonce32, pub boot_id: BootId, pub generation: u64,
    pub config_hash: Hash32, pub route_state_hash: Hash32, pub computed_at: Timestamp,
    pub config: ConfigLines, pub routes: RouteLines }
pub struct IncompleteReport { pub helper_key_id: String, pub nonce: Nonce32, pub boot_id: BootId,
    pub reason: IncompleteReason, pub computed_at: Timestamp }
pub fn render_complete(report: &CompleteReport) -> Vec<u8>;
pub fn render_incomplete(report: &IncompleteReport) -> Vec<u8>;
pub fn parse_report(bytes: &[u8]) -> Result<Report, ReportError>;       // strict, never re-serialises
impl CompleteReport { pub fn check_hashes(&self) -> Result<(), ReportError>; }  // recompute both from the report's own lines
pub struct IncompleteReason(String);   // token: [a-z][a-z0-9_]{0,63}; the signed set is the table in 6.4
```

**Strict parser rules** (every one has a named test, 12.3). `parse_report(bytes)` rejects, each with its own
`ReportError` variant: bytes that are not valid UTF-8 (its own step, run first, D-52); a BOM; any CR; a blank
line; trailing whitespace; a missing final LF; an unknown, missing, duplicate or out-of-order key; unsorted or
duplicated repeated lines; a non-canonical value (leading zeros, uppercase IPv6, host bits set in a `cidr`, lowercase
percent hex, an encoded literal); `format` other than `1`; a hash that is not 64 lowercase hex; a `boot_id` that is
not lowercase `8-4-4-4-12` hex; a `computed_at` that is not `YYYY-MM-DDTHH:MM:SSZ` of a valid calendar time; an empty
value anywhere except `incomplete_reason`; an `incomplete_reason` that is empty on `complete=false` or non-empty on
`complete=true`; a `complete=true` report whose `generation` is 0 or whose either hash is the zero hash; a
`complete=false` report whose shape is not exactly the minimal shape of 6.4 (generation 0, both hashes zero,
`max_ttl_s=0`, `max_active=0`, no repeated line); a report over `MAX_REPORT_BYTES`. A parsed report satisfies
`render(parse(x)) == x` byte for byte (the canonical-iff-round-trip property).

**Identity** (`identity.rs`, **fixed**):

```rust
pub struct SafetyIdentity { pub fingerprint: KeyFingerprint, pub boot_id: BootId, pub generation: u64,
                            pub config_hash: Hash32, pub route_state_hash: Hash32 }
impl SafetyIdentity {
    /// "v1:<fingerprint 64 hex>:<boot_id>:<generation>:<config_hash 64 hex>:<route_state_hash 64 hex>"
    /// The text stored in Authorization.safety_identity and, as evidence, in a path confirmation.
    pub fn canonical(&self) -> String;
    pub fn parse_canonical(text: &str) -> Result<Self, IdentityError>;   // strict: lowercase, exact field count, no surrounding space
}
pub enum IdentityMatch { Exact, BootOnlyDiffers, Different }
pub fn compare(stored: &SafetyIdentity, current: &SafetyIdentity) -> IdentityMatch;
```

`Nonce32` is a core newtype over `[u8; 32]` (the proto `Nonce` converts to and from it). `BootId` is a validated newtype (`8-4-4-4-12` lowercase hex, 36 bytes); it is **not** `uuid::Uuid`'s serde, which
accepts uppercase, hyphenless and braced forms. `UnverifiedReason` (`reason.rs`) is the closed nine, serde
`snake_case`, no unknown-value preservation (as `AuthorizationKind`): `key_not_pinned`, `key_mismatch`,
`attestation_missing`, `attestation_stale`, `signature_invalid`, `report_incomplete`, `generation_regressed`,
`path_uncovered`, `confirmation_stale`; with `token()`, `parse()` and a one-to-one mapping with
`UnverifiableReason` except `NotAvailable` (internal; maps to `attestation_missing` on the wire).

### 6.2 Grammars (**fixed**; amended readings of the harness, `04b` §1 to §3)

```abnf
literal   = ALPHA / DIGIT / "." / "_" / ":" / "/" / "@" / "-"
pct       = "%" 2(DIGIT / %x41-46)            ; uppercase hex only; MUST NOT encode a byte that is in `literal`
field     = 1*(literal / pct)                  ; decoded: 1 or more octets
uint      = "0" / (%x31-39 *DIGIT)             ; canonical decimal, range per use
family    = "inet" / "inet6"
hash64    = 64(DIGIT / %x61-66)                ; lowercase
hex16     = 16(DIGIT / %x61-66)
boot-id   = 8HEXLC "-" 4HEXLC "-" 4HEXLC "-" 4HEXLC "-" 12HEXLC
timestamp = 4DIGIT "-" 2DIGIT "-" 2DIGIT "T" 2DIGIT ":" 2DIGIT ":" 2DIGIT "Z"    ; valid calendar time
token     = %x61-7A *63(%x61-7A / DIGIT / "_")  ; incomplete_reason, 1..64 chars
ifname    = 1*15OCTET                           ; decoded octets, carried as bytes everywhere
unit      = 1*255(ALPHA / DIGIT / ":" / "_" / "." / "@" / "-")   ; known type suffix: service socket target mount automount swap timer path slice scope device; no backslash
container = (ALPHA / DIGIT) *254(ALPHA / DIGIT / "_" / "." / "-")
max-ttl   = 1*DIGIT ("s" / "m" / "h" / "d")     ; lowercase, no sign, no sum, value >= 1, seconds fit u64
local-addr= ifaddr "|" field                    ; route = family "|" u32 "|" cidr "|" (ip / "-") "|" field "|" u32
rule      = family "|" u32 "|" selector-f "|" u32
endpoint  = ip "|" u32 "|" (ip / "-") "|" field "|" ip      ; link-peer = ip "|" port
selector  = "all" / item *(";" item)            ; fixed order, each at most once
item      = "from=" cidr / "to=" cidr / "iif=" field / "oif=" field            ; field = strict-encoded name (inner escaping)
          / "fwmark=0x" hexn "/0x" hexn / "ipproto=" uint8 / "sport=" range16
          / "dport=" range16 / "uidrange=" range32 / "not"
hexn      = "0" / (%x31-39 / %x61-66) *7HEXLC   ; lowercase, no leading zeros, at most 8 digits
range16   = uint "-" uint                       ; each <= 65535, start <= end;  range32 likewise to 4294967295
```

Values: `table`, `priority`, `metric` are canonical `u32`; `port` is 1 to 65535; `ipproto` 0 to 255; `max_active` a
non-negative integer, `0` meaning no containment, default `0`; `max_ttl` default `3600` s when missing, `0` refused
as `config_invalid`. Unknown keys under `[safety]` and `[safety.containment]` are refused (`config_invalid`); a
missing `[safety]` equals an empty one. Hash fields in `expected_safety` and `fingerprint` are 64 lowercase hex;
`boot_id` is the `boot-id` form. The two belief checks are settled (synthesis 06 §5): the kernel refuses `:` and
`%` in an interface name and accepts `;`, `|`, `=`, `,` and a backslash; the unit grammar excludes the backslash
until the owner widens it, so an escaped unit name is `config_invalid`.

### 6.3 Hashes, signature input and the frozen vectors (**fixed**)

- `config_hash` = SHA-256 of `CONFIG_PREFIX` followed by the config lines (`cidr*`, `protected_unit*`,
  `protected_container*`, `max_ttl_s`, `max_active`), each LF-terminated. `route_state_hash` likewise with
  `ROUTES_PREFIX` and the route lines (`hub_addr*`, `link_peer*`, `local_addr*`, `route*`, `rule*`,
  `endpoint_route*`). Excluded from both: `format`, `helper_key_id`, `nonce`, `boot_id`, `generation`, the hashes,
  `complete`, `incomplete_reason`, `computed_at`. The signed bytes are `SIGNING_PREFIX` followed by the full report
  bytes. The three prefixes are distinct; a bare-report signature must not verify (test).
- The vectors **C1 to C4 and R1 to R4** of `docs/rework/04b-attestation-encoding.md` §4 are the contract. Frozen
  digests (an implementation must reproduce every one; lengths in bytes): C1 and C2 `96f1584f01f025fa82b1e57c3ebf8d220b87d6d88543d08add3dc273cbf4b9ea`
  (320, equal on purpose), C3 `494a37a86856bb2b2f6e80e13efc654e8b7fbaf9e73245277f6d34993d42dc4f` (320), C4
  `7f1db0e04eb12b4211166be055486a71e44be9e00c2a089cc9e51f4f06556baa` (198), R1
  `082d1afdf657096d1b3a97215142239f6910b74f13f158fbdda7f4d9fd4d1515` (583), R2
  `7eb46fcc67a00328df9e33578f42a4a890e6797223d6676a162c0b4ca1777ad9` (585), R3
  `7947b31b2c89edab797c3f6a1251b42ca4452d64d06dd786d68570d721996ec8` (25), R4
  `530d9a9e286ca63ee69147d0a7e0326bceb4199f114824f1969e4126e7ac925a` (123). The input texts are **not** repeated here;
  copy the byte-exact files from `docs/rework/investigations/probes/r10/fixtures/vectors/` (`c1_input.toml` ...
  `c4_input.toml`, `c1.txt` ... `c4.txt`, `r1.txt` ... `r4.txt`) and `fixtures/invalid_lines.txt`. R1 is an encoding
  vector, not an output of the selection rule, and R4 uses a name the kernel would refuse; both are kept as
  encoding vectors only (`04b` §4), and the selection logic gets its own semantic vectors (12.4).
- **New vectors to add and freeze** (synthesis 03 §7.4): the selector escaping lines of `04b` §4 (`br-lan`, `a;not`,
  `a=b` and `oif` equivalents, plus `%`, a non-ASCII byte pair and a 15-byte `;` name); a full complete report and a
  minimal incomplete report; both signed with a **published test-only seed** (Ed25519 signatures are
  deterministic, so the signature bytes are frozen); a valid state file and each corrupt variant; `expected_safety`
  JSON for both tags and every refused form; IPv6 corner forms (a single zero group, an IPv4-mapped address) with
  expected canonical text; and the semantic vectors (a route and rule snapshot to expected route lines).

### 6.4 Report shapes and the token tables (**fixed**)

Complete report: exactly the RW-37 layout (`format, helper_key_id, nonce, boot_id, generation, config_hash,
route_state_hash, complete, incomplete_reason, computed_at`, then config lines, then route lines), with
`incomplete_reason=` empty and `complete=true`. Incomplete report, the only shape of `complete=false`:

```text
format=1
helper_key_id=<hex16>
nonce=<hex64>
boot_id=<boot-id>
generation=0
config_hash=<64 zeros>
route_state_hash=<64 zeros>
complete=false
incomplete_reason=<token>
computed_at=<timestamp>
max_ttl_s=0
max_active=0
```

**Signed incomplete reasons** (`incomplete_reason`; the helper holds a usable key and signs the minimal report):

| Token | Condition |
|---|---|
| `state_missing` | key present, `state` absent |
| `state_corrupt` | checksum, parse or `format` failure |
| `state_key_mismatch` | state's `key_fingerprint` is not the key's |
| `config_invalid` | `helper.toml` parses but a value fails its grammar, an unknown `[safety]` key, `max_ttl=0` |
| `config_unreadable` | `helper.toml` or `agent.toml` cannot be read or parsed as TOML |
| `rules_unreadable`, `table_unreadable`, `addresses_unreadable`, `lookup_failed` | the matching `ip -j` read failed, timed out or was truncated |
| `route_read_partial` | a read returned output the parser could not complete (missing required key, truncated) |
| `route_unrepresentable` | a policy rule whose action is not lookup, a selected route whose type is not unicast, a multipath route, or a non-UTF-8 interface name (iproute2 JSON cannot carry one; ADR 0032 E3 resolved this way) |
| `state_write_failed` | the new state could not be made durable (no generation change is visible to the hub) |
| `report_too_large` | the complete report would exceed `MAX_REPORT_BYTES` |

**Unsigned closed tokens** (no usable key or no safe access; the helper returns the token without a signature, the
agent relays it as an `error` frame whose `code` is the token, the hub gates on `attestation_missing`):
`uninitialized` (neither key nor state), `key_missing` (state present, key absent), `key_corrupt` (key file not a
32-byte Ed25519 seed), `helper_state_permissions` (directory, key, state or lock has a wrong owner, mode or type;
the same token 47a's start check reports), `lock_unavailable` (the lock file cannot be opened or locked). Agent
originated: `helper_unavailable` (no helper, socket unreachable, group `safety.attest` off, timeout). An unsigned
token is an unverified hint a compromised agent can invent; it can unlock nothing.

**Wire reasons** (`unverified.reason`): the nine of 6.1. **Hub-internal `detail`** (audit and logs only, never on
the wire, no wire enum growth): `bad_encoding`, `hash_mismatch`, `report_malformed`, `format_unsupported`,
`nonce_unknown`, `nonce_other_host`, `nonce_expired`, `generation_zero`, `generation_repeated`,
`helper_incomplete:<token>`, `attestation_unsigned:<token>`, `link_timeout`, `link_unavailable`,
`agent_unsupported`.

### 6.5 Helper side (`<helper>/attest/`; **fixed** behaviour, **convention** module split)

#### 6.5.1 Configuration to lines

`domovoi-config` parses `[safety]` (`management_cidrs`, `protected_units`, `protected_containers`) and
`[safety.containment]` (`max_ttl`, `max_active`) with `deny_unknown_fields` into `SafetyInput`; core
`ConfigLines::effective` validates and canonicalises. Effective set: `cidr` lines are the configured CIDRs
normalised and de-duplicated; `protected_unit` lines are the configured units canonicalised and **unioned with
`MANDATORY_UNITS`**; `protected_container` lines are names (ids resolve at apply time); comments, whitespace, key
order and duplicates do not affect the hash. The mandatory list keeps `sshd.service` literally (vectors frozen);
matching by unit `Id` and `Names` is 45's concern. A parse or validation error yields **no hash**: the helper
signs the minimal incomplete report with `config_invalid` or `config_unreadable`.

#### 6.5.2 State, lock, atomic write (RW-37 "Helper state, recovery and concurrency"; ADR 0032 decisions 8 and 9)

Directory `/var/lib/domovoi-helper` (root, 0700), `attest.key` (0600, the 32-byte Ed25519 seed), `state` (0600),
`state.lock` (0600, empty), `retired/<ts>/` (root, 0700). Use the paths and modes 47a created.

```text
format=1
key_fingerprint=<64 hex>
generation=<u64 decimal>
config_hash=<64 hex>
route_state_hash=<64 hex>
boot_id=<boot-id>
checksum=<64 hex SHA-256 of every preceding byte, LF included>
```

Strict parse: each line LF-terminated, keys in this order, no extra line; any deviation is `state_corrupt`.
Every `safety.attest` takes `File::lock` (exclusive) on `state.lock` and holds it across read, recompute, compare,
increment, write and sign. Write: `state.tmp` (create, write, `sync_all`), `rename` over `state`, `sync_all` on the
directory, all **inside the lock and before the report that names the new generation is signed**. A leftover
`state.tmp` is deleted under the lock at the start of a request. A write happens only when the hashes changed or
`boot_id` differs from the persisted one; generation increments by exactly one only when the hashes changed, never
for a new `boot_id` alone and never for nft set membership (membership is not an input). A change followed by its
revert advances the generation by two. Missing, corrupt or mismatching state is **never reset automatically**.

#### 6.5.3 Key lifecycle (**fixed**; local root commands, never run implicitly)

| Command | Behaviour |
|---|---|
| `domovoi helper init` | root only; refuses if `attest.key` or `state` exists; creates the directory (0700), the key by exclusive create (0600, `sync_all` of file and directory), the lock, then `state` with `generation=0`, both hashes zero and the current `boot_id`; prints the fingerprint (groups of four) and the 16-character key id. Needs no valid configuration and no observer. A crash between key and state leaves a key without state; `recover` is the way out |
| `domovoi helper rotate-key` | needs key and state; writes `retired/<ts>/` (old public key, its fingerprint text, a copy of the old `state`) durably first, then the new key and a new state (`generation=0`, zero hashes), both durable, then replaces the live files; the **old private key is deleted** (no copy anywhere in the tree). The sequence restarts under the new key |
| `domovoi helper recover` | for lost or damaged state; needs at least one of the two files; same retirement and deletion as `rotate-key`; **always a new key** (continuity cannot be proven after state loss) |
| `domovoi helper fingerprint` | computes from the private key file and prints the 64 hex in groups of four plus the key id; root only |

`<ts>` is UTC `YYYYMMDDTHHMMSSZ` from the `Clock` port. A crash mid-rotation leaves either the old pair or the new
pair plus a possible key/state mismatch (`state_key_mismatch`, fail closed); `recover` resolves it. Each
generation sequence is per key; a rotated or recovered helper presents a new public key, so the hub shows "helper
key changed" until the owner re-enrols (6.7.6).

#### 6.5.4 `safety.attest` (**fixed**)

Op name `safety.attest`, behind group `safety.attest`. Request: `nonce` as 64 lowercase hex (the 32 raw nonce
bytes; the nonce is the helper's only input). Response: `Signed { public_key: [u8; 32], report: Vec<u8>, signature:
[u8; 64] }` or `Unsigned { token }` (the unsigned tokens of 6.4). Algorithm:

```text
check directory and files as 47a's start check (descriptors, no symlink)       else Unsigned(helper_state_permissions)
                                                                                // an ABSENT `state` or `attest.key` is "absent", not a wrong mode: it falls through to the signed state_missing / unsigned key_missing below
open state.lock, lock exclusive                                                 else Unsigned(lock_unavailable)
delete leftover state.tmp
key   = load_key()                                                              else Unsigned(uninitialized | key_missing | key_corrupt)
st    = load_state()                                                            Err(token) => signed_incomplete(token)
if st.key_fingerprint != fingerprint(key)                                       => signed_incomplete(state_key_mismatch)
cfg   = ConfigLines::effective(read helper.toml)                                Err => signed_incomplete(config_invalid | config_unreadable)
obs   = observe(endpoints(cfg))                                                 Err(e) => signed_incomplete(e.token())
report = render_complete(nonce, boot_id(), cfg, obs, generation_for(st, hashes))
if len(report) > MAX_REPORT_BYTES                                               => signed_incomplete(report_too_large)
if hashes != st.hashes or boot_id != st.boot_id:
    write_state_atomic(next)                                                    Err => signed_incomplete(state_write_failed)
return Signed(public_key, report, sign(key, signing_bytes(report)))
```

`boot_id` is read from `/proc/sys/kernel/random/boot_id` through the `HostFs` port (`ports/hostfs.rs`), lowercase
form. `computed_at` comes from the `Clock` port at second precision (display only; the hub never trusts it). The
signing key type is constructed **only** in `keys.rs` and `lifecycle.rs` (`ATTEST-9` neighbour: no other module
calls `SigningKey::from_bytes` or generates a key).

#### 6.5.5 Route observation (`observe.rs`, `ip_source.rs`, `ip_json.rs`; **fixed** semantics, **convention** names)

The harness's `RouteSource` port is kept (async here because `ProcessExecutor` is async):

```rust
#[async_trait] pub trait RouteSource: Send + Sync {
    async fn rules(&self) -> Result<Vec<PolicyRule>, ReadError>;          // both families
    async fn table_routes(&self, table: u32) -> Result<Vec<TableRoute>, ReadError>;
    async fn addresses(&self) -> Result<Vec<IfAddress>, ReadError>;
    async fn lookup(&self, endpoint: IpAddr) -> Result<Lookup, ReadError>; // kernel lookup honouring policy rules
}
pub async fn observe(src: &dyn RouteSource, ep: &Endpoints) -> Result<RouteLines, ReadError>;
```

`observe` is the harness's selection logic, with these amendments: `rule` lines are every lookup rule; **any rule
whose action is not lookup is `route_unrepresentable`**, and any selected route whose `type` is not `unicast`, any
multipath route and any interface name that is not UTF-8 is `route_unrepresentable` too (D-52, G-8, never a hash
that omits the item); referenced tables are the rule tables plus `main` (254) and `local` (255); `route` lines are
the routes the endpoint lookups selected plus every default route (prefix length 0) of a referenced table;
`local_addr` lines are the addresses on interfaces an `endpoint_route` names; endpoints are every `hub_addr`, every
`link_peer` address and the **network address** of every configured `cidr`. Known limits stated, not fixed: a more
specific route inside a configured CIDR is invisible (G-9); a route change between the last read and an apply is
the accepted kernel race (G-20).

**Production source: `ip -j` through `ProcessExecutor`** (the same CLI-over-netlink choice as `nft`, D-56; R3 and
R4: argv vector, no shell, complete environment `{"LC_ALL": "C"}`, timeout 5 s per call, `max_output_bytes` 4 MiB;
a truncated or timed-out read is a failed read). The program is the first existing of `/usr/sbin/ip`, `/sbin/ip`,
`/usr/bin/ip`, `/bin/ip` (checked with `HostFs`); none exists gives `rules_unreadable`.

| Read | argv after the program |
|---|---|
| rules | `-N -j -4 rule show`, `-N -j -6 rule show` |
| a table's routes | `-N -j -4 route show table <T>`, `-N -j -6 route show table <T>` |
| addresses | `-N -j addr show` |
| an endpoint lookup | `-N -j -d -4 route get <ip>` or `-N -j -d -6 route get <ip>` (family of the endpoint) |

The flags `-N` (numeric names) and `-d` (details, so the selected `table` and the route `type` are printed) are the
intended way to obtain a numeric table and the type. **Caveat:** the iproute2 manual says `-Numeric` prints protocol,
scope, dsfield and **type** as numbers, so `type` may arrive as `1` (`RTN_UNICAST`) rather than `"unicast"`; the
parser accepts exactly `"unicast"` or `1` as unicast and refuses everything else, whatever form step 1 shows.
**Step 1 of section 7 captures real output and confirms the key names, the form of `type`, that `-N` yields numeric
tables, and that `table` is present on `route get`**. If a lookup does not print its table, do not guess: the
lookup fails `lookup_failed` and you stop and report, because the design then needs the fallback of deriving the
table from the rule and table dumps, which is an owner decision. `ip_json.rs` is a pure function from the JSON text
to the typed values (`parse_rules`, `parse_routes`, `parse_addresses`, `parse_lookup`); it reads only the documented
keys, rejects (does not ignore) a rule it cannot map completely, and is tested against captured fixtures with
documentation-range addresses (`192.0.2.0/24`, `198.51.100.0/24`, `2001:db8::/32`) under `<helper>/tests/fixtures/ip/`.
Live reads are host-only (12.5).

#### 6.5.6 Endpoints: where `hub_addr` and `link_peer` come from (ADR 0032 E5; **default pending owner confirmation**)

The owner has not decided the source. The default built here, the most conservative one the rules allow, and the
launch gate item to confirm or replace:

- `hub_addr` lines: the **literal IP address** in the host part of `AgentConfig.hub_url` read from the root-owned
  `/etc/domovoi/agent.toml` by the helper (the agent cannot alter that file). A hub URL that names a host
  (not an IP literal) yields **no** `hub_addr` line: resolving a name would churn the generation with DNS, and a
  name the agent controls must not decide an exclusion. The Helper tab states this when the report has no
  `hub_addr` line, and the book says to cover the hub's address with a `management_cidrs` entry.
- `link_peer` lines: **none in this release**. The report cannot express routing inside a configured CIDR and the
  helper has no trustworthy view of the agent's connections (E5 stays open for 46).
- The source is behind one trait, `EndpointSource { fn hub_addrs(&self) -> Result<Vec<IpAddr>, ReadError>; fn
  link_peers(&self) -> Result<Vec<(IpAddr, u16)>, ReadError>; }`, so replacing the default touches one file.

#### 6.5.7 What goes wrong at the helper (tokens)

| Condition | Result |
|---|---|
| directory, key, state or lock wrong owner, mode or type | `Unsigned(helper_state_permissions)` |
| lock cannot be opened or locked | `Unsigned(lock_unavailable)` (never `state_corrupt`, M F-6) |
| both key and state missing | `Unsigned(uninitialized)` |
| key missing, state present | `Unsigned(key_missing)`; the helper **never** generates a key |
| key not 32 bytes | `Unsigned(key_corrupt)` |
| state missing / corrupt / key mismatch | signed incomplete `state_missing` / `state_corrupt` / `state_key_mismatch` |
| write interrupted | old file intact; generation not advanced; retried next request; never decremented |

#### 6.5.8 Apply-time guard, the part 47c owns (`guard.rs`, pure)

```rust
pub enum GuardMode { Compared, SkippedUnverified }
pub enum GuardRefusal { MalformedSafety, SafetyChanged, SafetyUnknown }
/// The identity half of apply-time enforcement (RW-37 steps 1 and 2). `expected` is the request's tag as received.
pub fn check_expected(expected: Option<&ExpectedSafety>, current: Result<&SafetyIdentity, ComputeFailure>)
    -> Result<GuardMode, GuardRefusal>;
```

Order: an absent tag is `MalformedSafety` (absence is never read as "no guard", D-51); an uncomputable current
identity is `SafetyUnknown`; `Verified{..}` is compared on all five fields (own fingerprint, `boot_id`,
`generation`, both hashes) and unequal is `SafetyChanged`; `Unverified{..}` skips only the comparison. Overlap,
protected units, TTL, `max_active`, `no_management_cidrs` and the mutation limit are apply-time containment
checks and belong to 46 and 45 (12.2).

### 6.6 Agent relay (`crates/domovoi-agent/src/requests/attest.rs`; **fixed**)

Add `Message::AttestSafety` to the dispatch (`link/mod.rs:425-455`; today it falls to `Unknown` and is answered
`unsupported`). The handler calls 47a's helper client with `safety.attest{nonce}` and replies:

- `Signed` to `Message::SafetyAttestation { public_key, report, signature }`, the report and signature **bytes
  unchanged** (never parsed, re-serialised or filtered by the agent);
- `Unsigned{token}` or any helper failure to `Message::Error(ErrorMessage { code: <token>, message: <short fixed
  text> })` with `reply_to` the request id, so the hub does not wait for a timeout. A missing helper or an
  unreachable socket is code `helper_unavailable`. No new frame kind and no enum growth.

The agent also announces the helper's public key: `hello.helper_identity` and `agent_status.helper_identity` carry
`HelperIdentity { public_key }` when the helper answers (the agent asks the helper for its **public** key through
47a's status or a `safety.public_key` read; 47a offers neither (its `Health` answer carries protocol, build and pid only), so add the read-only op `safety.public_key`
only if none exists). The agent sends no fingerprint.

### 6.7 Hub side (`crates/domovoi-hub/src/helper_trust/`; **fixed** behaviour)

#### 6.7.1 Link method and the stored received key

```rust
// ports/agent_link.rs, default method (the 42 pattern): Err(LinkError::Transport(NOT_SUPPORTED))
async fn attest_safety(&self, host: HostId, nonce: Nonce32, deadline: Timestamp)
    -> Result<AttestationReply, LinkError>;
pub enum AttestationReply { Signed { public_key: [u8; 32], report: Vec<u8>, signature: [u8; 64] },
                            Unsigned { token: String } }
```

`WsAgentLink` sends `attest_safety` and maps `safety_attestation` to `Signed`, an `error` reply to
`Unsigned{ token: code }`, `unsupported` to `Unsigned{ token: "agent_unsupported" }`; a timeout is
`LinkError::Timeout` (no outcome change, nothing is revoked by a missing reply). `Live` gains
`helper_identity: Option<[u8; 32]>` stored from `hello` and updated from every `agent_status`
(`agent_server/live.rs:44`, `session.rs:49`); the hub computes `KeyFingerprint::of_public_key` itself and never
reads a fingerprint field from an agent.

#### 6.7.2 Nonces

`NonceStore`: in memory (a restart merely forces a re-request; X-12). `issue(host, now) -> Nonce32` draws 32
bytes from the OS CSPRNG through the same `rand` call the agent challenge already uses (`agent_server`), stores
`(host, nonce, issued_at)` and drops entries older than `NONCE_TTL` on every call. `take(host, nonce, now) ->
Result<Timestamp /* issued_at */, NonceError>`: unknown nonce is `Unknown`, another host's is `OtherHost`, an age
of `NONCE_TTL` or more is `Expired`; success removes it. Boundaries are strict. All times come from the `Clock`
port.

#### 6.7.3 Verification (the only place a safety report is verified; `verify.rs`)

| Step | Check | Failure: wire reason / detail | Nonce |
|---|---|---|---|
| 1 | decode: `public_key` exactly 32 bytes, `report` at most `MAX_REPORT_BYTES`, `signature` exactly 64 | `signature_invalid` / `bad_encoding` | kept |
| 2 | pin lookup: none | `key_not_pinned` | kept |
| 2 | received key differs from the pin (byte compare) | `key_mismatch`, audit `helper.key_mismatch`, latest outcome becomes mismatch | kept |
| 3 | `verify_strict(pin, signing_bytes(report), signature)` over the **received bytes** | `signature_invalid` | kept |
| 4 | `parse_report` on the same bytes | `report_incomplete` / `report_malformed` or `format_unsupported` | kept |
| 5 | complete: recompute both hashes from the report's own lines | `report_incomplete` / `hash_mismatch` | kept |
| 5 | incomplete: shape equals the minimal shape (the parser already enforced it) | `report_incomplete` / `report_malformed` | kept |
| 6 | nonce found, same host, age under `NONCE_TTL` (**consumed here, late**) | `attestation_missing` / `nonce_unknown`, `nonce_other_host`, `nonce_expired` | consumed once found |
| 7 | valid and incomplete: record the reason token and the receipt time for display; revoke verified state | `report_incomplete` / `helper_incomplete:<token>` | consumed |
| 8 | `generation >= 1` | `report_incomplete` / `generation_zero` | consumed |
| 9 | `generation < high-water` for `(host, fingerprint)` | `generation_regressed`; revokes | consumed |
| 10 | `generation == high-water` and either hash differs from the stored | `generation_regressed` / `generation_repeated`; revokes | consumed |
| 11 | accept: in **one** transaction store report bytes, identity and the nonce **issue** time, set the high-water mark, set latest outcome to verified | verified | consumed |

`helper_key_id` is a label: never cross-checked, never used to verify or decide. A forged reply does not burn the
outstanding nonce (step 6 comes after steps 1 to 5). Only a message that passes steps 1 to 5 can reach steps 6 to
11. The one outcome set before the signature is checked is the key mismatch of step 2: it sets the latest outcome
to mismatch (and revokes), because a different key is evidence the helper changed or the agent substituted one;
the next verified attestation under the pin clears it, so a forged key blocks only until one honest relay.

#### 6.7.4 Latest-outcome gate (**fixed**)

`LatestOutcome`, `TrustView`, `VerifiedState`, `PinnedKey`, `gate` and its companions live in `domovoi_core::attest::gate`
(pure; the engine's `HelperTrustRead` returns them and core's `gate` takes them); the hub's `gate_view.rs` only builds
them from the store and the live session.

```rust
pub enum LatestOutcome { Verified, Mismatch, SignedIncomplete, Regressed }
pub struct TrustView { pub pin: Option<PinnedKey>, pub received_key: Option<[u8; 32]>,
                       pub latest: Option<LatestOutcome>, pub verified: Option<VerifiedState> }
pub struct VerifiedState { pub identity: SafetyIdentity, pub report: Vec<u8>, pub nonce_issued_at: Timestamp,
                           pub accepted_at: Timestamp, pub max_ttl_s: u64, pub max_active: u64,
                           pub cidrs: Vec<Cidr>, pub hub_addrs: Vec<IpAddr>, pub link_peers: Vec<(IpAddr, u16)> }
pub fn gate(now: Timestamp, view: &TrustView) -> Result<&VerifiedState, UnverifiedReason>;
```

Order: no pin gives `key_not_pinned`; a received key differing from the pin, or latest `Mismatch`, gives
`key_mismatch`; latest `SignedIncomplete` gives `report_incomplete`; latest `Regressed` gives
`generation_regressed`; no stored verified state gives `attestation_missing`; `now - nonce_issued_at >=
FRESHNESS` gives `attestation_stale`. A verified identity is stored **only** by step 11 (`ATTEST-2`). A key
mismatch, a validly signed incomplete report or a generation regression **revokes** the stored verified state
(clears it); one later good attestation restores trust; unauthenticated failures revoke nothing. There is **no
per-host sticky flag** (`ATTEST-7`). Pure companions in core:

```rust
pub fn covered(state: &VerifiedState, cidr: &Cidr) -> bool;   // subset of some cidr line, or equals a hub_addr or link_peer address; agent-reported data never counts
pub fn confirmation_current(stored: &SafetyIdentity, confirmed_at: Timestamp, current: &VerifiedState, now: Timestamp)
    -> Result<(), UnverifiedReason>;                           // Exact, or BootOnlyDiffers with a fresh verified current; age under CONFIRMATION_MAX_AGE; else ConfirmationStale
pub fn permit_identity_current(stored: &SafetyIdentity, current: &SafetyIdentity) -> bool;  // all five fields equal, boot_id included (never the reboot exception)
```

#### 6.7.5 When the hub asks, and the request path (`request.rs`, `schedule.rs`)

The hub requests an attestation on an authenticated `hello` (compose with the existing host reconciler, as 42 did:
read `runtime/mod.rs` for the current reconciler composition), on `agent_status`, every 15 minutes for each
connected host whose helper reports present, on the owner's Re-check, and on demand through
`HelperTrust::fresh(host)` for 46's permit issuance. One in-flight request per host (a per-host guard). A host
without a pin is not asked (the answer is `key_not_pinned` and the received key is shown for enrolment). The
procedure is: pin lookup, `NonceStore::issue`, `AgentLink::attest_safety`, `verify`, one unit-of-work commit
(`StateWrite::HelperTrust`) with its audit rows. Audit rows (08/09): `helper.key_pinned`, `helper.key_rotated`
(old and new fingerprints), `helper.key_unpinned`, `helper.key_mismatch`, `helper.attestation_revoked`,
`helper.attestation_restored`, `helper.generation_regressed`, `helper.high_water_reset`. Transitions are audited,
**not every successful 15-minute attestation**. No audit row carries report bytes, a signature or a nonce.

#### 6.7.6 Pinning and enrolment (`pin.rs`, `services/helper_trust.rs`; all commands take `&Caller`)

```rust
impl HelperTrust {
    pub async fn view(&self, host: HostName) -> Result<HelperView, HelperError>;
    pub async fn recheck(&self, caller: &Caller, host: HostName) -> Result<(), HelperError>;
    pub async fn pin(&self, caller: &Caller, host: HostName, typed_fingerprint: &str, password: &SecretString)
        -> Result<HelperView, HelperError>;
    pub async fn unpin(&self, caller: &Caller, host: HostName, password: &SecretString) -> Result<HelperView, HelperError>;
}
```

`pin` needs a session, the CSRF token (handler), the account password (`StepUp`, the package service's limiter) and
the **full 64-hex fingerprint typed by the owner** (spaces and case ignored); it pins the key the hub **currently
holds from the helper** for that host, only when the typed value equals `KeyFingerprint::of_public_key` of that key
(constant-time compare, never the key id). The owner reads the printed value on the host with `domovoi helper
fingerprint`. Nothing is pinned automatically, including on first contact. In **one transaction**: insert or
replace `helper_keys`; delete the `helper_high_water` row for `(host, fingerprint)` (this is the owner re-pin that
resets the mark, audited `helper.high_water_reset` when a row existed); clear the verified state; write the audit
row (`helper.key_pinned`, or `helper.key_rotated` when a pin existed). `unpin` clears the pin and the verified
state, keeps the high-water row, audits `helper.key_unpinned`; containment stays blocked (`key_not_pinned`). A
host removed from the hub loses its pin, state and high-water rows in the same transaction as the host. Add
`helper.key_pin` and `helper.key_unpin` to `STEP_UP_ENFORCED_ACTIONS` (`settings/step_up.rs:103`, `enforced_by:
"HelperTrust"`); the typed fingerprint is the "typed confirmation" of the approval model.

### 6.8 Wire changes (`crates/domovoi-proto`, folded into unreleased 1.2; **fixed**)

- `ExpectedSafety` moves to `domovoi_core::attest::expected` and is re-exported from `domovoi_proto::messages`.
  Shape: `#[serde(tag = "state", rename_all = "snake_case")]`; `Verified { fingerprint: Hash32, boot_id:
  BootId, generation: u64, config_hash: Hash32, route_state_hash: Hash32 }`; `Unverified { reason:
  UnverifiedReason }` (closed). An extra field, an unknown tag and an unknown reason are each **refused**. The
  mechanism is yours: serde's `deny_unknown_fields` on an internally tagged enum is not asserted to work, so the
  test `expected_safety_refuses_extra_field` decides, and a custom `Deserialize` through a private `deny_unknown_fields`
  struct per variant is allowed. An `Unverified` value serialises to exactly `state` and `reason` and carries no
  identity.
- `RunCapability.expected_safety` stays `Option<ExpectedSafety>` on the wire (other capabilities carry none). One
  validating function, used by the agent and by the helper's request decoder, enforces "required for `Contain`":
  `pub fn require_expected_safety(tag: Option<&ExpectedSafety>) -> Result<&ExpectedSafety, MalformedSafety>` in core
  (`attest::expected`). 46 calls it from the `Contain` request type; here it is built and tested, and the gate that
  forbids another route lands with 46.
- `SafetyAttestation` and `AttestSafety` keep their fields. `HelperIdentity` keeps `public_key` only.

### 6.9 Core `UnverifiableReason` and the coverage evidence (**fixed**)

`UnverifiableReason` (`knowledge/paths.rs:64`) gains `PathUncovered` ("the helper's attestation does not cover this
path") and `ConfirmationStale` ("the confirmation no longer matches the helper's current safety set"); each gets a
`describe` text; the mapping to `UnverifiedReason` is one to one except `NotAvailable` (to `attestation_missing`).
`CoverageEvidence` (`:10`) keeps its type, but its doc comment changes from "compared for equality only, never
parsed here" to "opaque to the knowledge store; the attestation module stores `SafetyIdentity::canonical()` in it
and parses it with `SafetyIdentity::parse_canonical`". `Covered { evidence }` carries
`CoverageEvidence::new(identity.canonical())` (the canonical text is 4 + 64 + 36 + 20 + 64 + 64 + 4 separators,
far below 2048 characters).

### 6.10 HTTP API (`/api/v1`, **fixed** paths and verbs, **convention** DTO names)

All routes resolve the host by name as `api/hosts.rs` does. Reads need a session; writes need a session and CSRF.

| Verb and path | Body | Result |
|---|---|---|
| `GET /hosts/{name}/helper` | none | `200 HelperDto` |
| `POST /hosts/{name}/helper/recheck` | none | `202`; `429 recheck_too_soon` if the last request for the host is under 10 s old by the hub clock |
| `PUT /hosts/{name}/helper/key` | `{ fingerprint: string, password: string }` | `200 HelperDto`; `409 no_helper_key` (the hub holds no key from this host); `422 fingerprint_mismatch`; `401 wrong_password`; `429` |
| `DELETE /hosts/{name}/helper/key` | `{ password: string }` | `200 HelperDto` |

`HelperDto` (all hub-computed, none copied from an agent field):

```text
host: string
helper: { state: "absent" | "ok" | "error", ops: string[], reason: string | null }       // as the agent reported: unverified text
received_key: { fingerprint: string, fingerprint_groups: string, key_id: string } | null
pin: { fingerprint: string, pinned_at: ts, pinned_by: string } | null
key_state: "no_helper" | "not_pinned" | "pinned" | "changed"                              // changed = received key differs from the pin
attestation: { state: "verified" | "stale" | "unverified", reason: UnverifiedReason | null,
               helper_reason: string | null,                                                // the SIGNED incomplete token (verified origin), display only
               unsigned_hint: string | null,                                                // the unsigned token the agent relayed (uninitialized, key_missing, key_corrupt, helper_state_permissions, lock_unavailable, helper_unavailable): an UNVERIFIED agent relay, the UI labels it so
               checked_at: ts | null }
verified: { generation, boot_id, config_hash, route_state_hash, nonce_issued_at: ts, age_seconds: int,
            fresh: bool, computed_at: ts, max_ttl_s, max_active, cidrs: string[], protected_units: string[],
            protected_containers: string[], hub_addrs: string[], link_peers: string[] } | null
```

`verified` is present only when the gate passes. Errors use the existing `ApiError` codes style with the new codes
above; the client shows text only through `describeError`. OpenAPI and `web/src/api/schema.d.ts` are regenerated
(never hand-edited).

### 6.11 Web (`web/src/features/capabilities/`; decided: endpoints **plus** a minimal real UI)

Once the hub receives and pins helper keys, the placeholder sentences ("This hub does not receive helper keys
yet") are false, so both placeholders are replaced, not kept. What ships (read `web/DESIGN.md` first; kit
primitives only; no inline `style`, no native `title`, no raw `<button>`; every file at most 250 lines):

- `HelperTab` renders `HelperPanel`: one `HelperHostCard` per host (from the existing hosts list) with the
  key state, the attestation state and reason (text from `helperText.ts`, statuses from `design/tone.ts`), the
  verified effective set read-only (CIDRs, protected units and containers, generation, freshness), a Re-check
  button, the note "edit `helper.toml` on the host and Re-check" and, when the verified report has no `hub_addr`
  line, the sentence about covering the hub's address. An unverified host shows the reason and the matching local
  action (a hint from `unsigned_hint` is marked "relayed by the agent, not verified") (`helper_state_permissions`: fix the mode; `state_missing`, `state_corrupt`, `state_key_mismatch`: run
  `domovoi helper recover`; `uninitialized`: run `domovoi helper init`; `key_missing`: run `domovoi helper recover`).
- `HelperKeyPage` (`/capabilities/helper-key`, optional `?host=`) renders `HelperKeyEnroll`: the received key's
  full fingerprint in groups of four marked **unverified** until pinned, the instruction to run `domovoi helper
  fingerprint` as root on the host and **type** the value, a password field, Pin; "helper key changed: re-enrol"
  (blocking, P0) when the received key differs from the pin; Unpin. Mutations use `useActionFeedback`.
- New statuses (`key not pinned`, `key changed`, `attestation stale`, `unverified`, `verified`) go into
  `design/tone.ts` and `helperVocab.ts`. Visibility ids: the two `placeholder` ids disappear; add
  `capabilities.helper.attestation` and `capabilities.helper-key.enroll`, both P0 (never hidden or collapsed).
- **Replace** `capability_center_has_no_helper_data_path` (`capabilityGates.test.ts:14`) with
  `helper_views_use_only_helper_endpoints` (the four endpoints of 6.10 are the only helper routes any web source
  names; no raw reason code or error sentence is written in a feature file) and `helper_views_never_claim_verified_without_a_verified_report`.
  Do not weaken it silently. Update the smoke mock routes, `screens.mjs` pages and `web/DESIGN.md` "Planned" text.
- Out of scope here: skill-runner status (47b), the unverified-human approval card (46), management-path
  confirmation UI (48's pages already show paths; their coverage badge becomes real through 6.12).

### 6.12 Coverage and engine (`domovoi-engine/src/knowledge/attested.rs`; **fixed**)

```rust
// ports/helper_trust.rs (core): implemented by the hub over the store and the live session
#[async_trait] pub trait HelperTrustRead: Send + Sync {
    async fn view(&self, host: HostId) -> Result<TrustView, StoreError>;
}
pub struct AttestedCoverage { trust: Arc<dyn HelperTrustRead>, clock: Arc<dyn Clock> }
impl ManagementCoverage for AttestedCoverage {
    fn is_available(&self) -> bool { true }
    async fn verdict(&self, host: HostId, cidr: &str) -> CoverageVerdict;
}
```

`verdict`: parse `cidr` (invalid is `Uncovered`); `gate(now, view)` error becomes `Unverifiable { reason }`;
`covered` false is `Uncovered`; otherwise `Covered { evidence: canonical identity }`. `ManagementCoverage` gains
`async fn confirmation_current(&self, host, evidence: &CoverageEvidence) -> Result<(), UnverifiableReason>`
(default method returning `Err(NotAvailable)`, fail closed; `UnavailableCoverage` and the testkit `FakeCoverage`
override explicitly) and `KnowledgeService` gains `confirmed_paths_with_currency(host)` for 46's permit issuance;
`path_state` and the existing path listing are **unchanged**. The hub wires `AttestedCoverage` in
`runtime/knowledge.rs:20` (and `examples/dev_hub.rs`); `UnavailableCoverage` stays for tests and as the wiring when
the store port is absent.

### 6.13 Decision functions (`attest/decide.rs`, pure; **fixed** names, consumed by 46)

The harness's decision functions are ported as pure functions so 46 only wires them to permits and approval cards:

```rust
pub enum Authority { Human, Policy }
pub enum Decision {
    Verified(SafetyIdentity),                                                    // gate passed, every candidate covered
    UnverifiedHuman { reason: UnverifiedReason, unprotected: Vec<Cidr> },        // human only: step-up and typed confirmation are 46's
    Refused(UnverifiedReason),                                                   // policy authority never proceeds on anything unverified
}
pub fn decide(now: Timestamp, view: &TrustView, authority: Authority, candidates: &[Cidr]) -> Decision;
pub fn unprotected_paths(view: Result<&VerifiedState, UnverifiedReason>, candidates: &[Cidr]) -> Vec<Cidr>;
pub fn agent_accepts(tag: &ExpectedSafety, authority: AuthorizationKind) -> Result<(), PolicyRequiresHuman>;
```

An uncovered candidate under a verified report gives `path_uncovered` (human: listed as unprotected; policy:
refused). An `Unverified` tag with `AuthorizationKind::Policy` is `PolicyRequiresHuman`, which 46 maps to the existing
`RefusalCode::PolicyRequiresVerifiedSafety` (`actions.rs:154`; RW-37's name `unverified_requires_human` is not added); no `PolicyPermit` is ever
built from an unverified value (46 enforces; the type cannot express it).

## 7. Ordered implementation steps

1. **Inspect and verify the repository context; report discrepancies.** Open every file and symbol of sections 3
   and 6. Read 47a's merged code and write down the 47a hand-off items. List `crates/domovoi-store/migrations/` and
   `docs/adr/` for the next numbers. **Capture real `ip -j` output** on a Linux host for each argv of 6.5.5 (a policy-routing
   host if available), confirm the JSON key names, that `-N` yields numeric tables and that `route get -d` prints
   `table`; scrub addresses to documentation ranges before committing any fixture. Write every difference down;
   stop if one changes the design (the `table` case of 6.5.5 and a 47a name mismatch are such cases).
2. **Core `attest`** (6.1 to 6.4, 6.9, 6.13): modules, constants, `UnverifiableReason` additions, the ports, the
   `AgentLink::attest_safety` default method. Unit and property tests; vectors frozen.
3. **Proto** (6.8): re-export, strict types, rewritten tests, new samples and snapshots. Review every snapshot.
4. **Config** (6.5.1): `[safety]` loader; C1 to C4 across config and core.
5. **Helper side** (6.5): keys, state, lock, compute, observe, `ip` source and parser, guard, lifecycle commands,
   `safety.attest` op wired into 47a's dispatch. Tests with fake `ProcessExecutor`, `HostFs` and `Clock`.
6. **Agent relay** (6.6): the `attest_safety` arm, `helper_identity` announcement, the `error` reply path.
7. **Store** (section 8): migration, `StateWrite::HelperTrust` applier and guards, `HelperTrustRead`, in-memory
   twin, host-removal purge. SQLite tests.
8. **Hub trust module and service** (6.7): link method, `Live.helper_identity`, nonce store, verifier, gate view,
   request path, schedule, pin and unpin, `services::HelperTrust`. Step-up registry entries. Hub tests.
9. **Engine coverage** (6.12): `AttestedCoverage`, `confirmation_current`, wiring in `runtime/knowledge.rs`;
   `monitoring_unaffected_by_missing_management_paths`.
10. **Arch rules and ADR** (section 11): `ATTEST-1` to `ATTEST-10`, `RAW_OPS`, zones; `cargo xtask arch-check` and
    `--base`.
11. **HTTP API** (6.10): routes, DTOs, error codes, OpenAPI regeneration; hub integration tests.
12. **Web** (6.11): hooks, views, vocab, visibility ids, replaced gate test, mock routes, smoke, `screens.mjs`.
13. **Privileged custody test and docs** (12.5, section 15); then the checks and the completion report.

## 8. Database and persistence

**Migration.** `crates/domovoi-store/migrations/00NN_helper_trust.sql`, `NN` the next unused number after listing
the directory on your branch (the last file seen is `0019_install_jobs.sql`; do not assume `0020`, and describe the
migration by its contents). STRICT tables, integer millisecond timestamps, hex and blob-length checks in the style of
`0019_install_jobs.sql`. **Enums (`latest_outcome`, outcomes, reasons) are validated in Rust, never by a SQL `CHECK`**;
the only `CHECK`s are hex and blob lengths and positive generations.

```sql
-- The key the owner pinned for each host's helper. One row per host; replaced only by an owner re-pin.
CREATE TABLE helper_keys (
    host_id     TEXT PRIMARY KEY,
    public_key  BLOB NOT NULL CHECK (length(public_key) = 32),
    fingerprint TEXT NOT NULL CHECK (length(fingerprint) = 64 AND fingerprint NOT GLOB '*[^0-9a-f]*'),
    pinned_at   INTEGER NOT NULL,
    pinned_by   TEXT NOT NULL
) STRICT;

-- The latest attestation outcome per host and, when the latest outcome is verified, the verified state.
CREATE TABLE helper_attestation (
    host_id          TEXT PRIMARY KEY,
    latest_outcome   TEXT NOT NULL,                 -- verified | mismatch | signed_incomplete | regressed (validated in Rust)
    latest_reason    TEXT,                          -- wire reason of the latest non-verified outcome
    latest_detail    TEXT,                          -- internal detail, e.g. helper_incomplete:state_corrupt
    latest_at        INTEGER NOT NULL,
    fingerprint      TEXT CHECK (fingerprint IS NULL OR (length(fingerprint) = 64 AND fingerprint NOT GLOB '*[^0-9a-f]*')),
    report           BLOB CHECK (report IS NULL OR length(report) <= 65536),
    generation       INTEGER CHECK (generation IS NULL OR generation >= 1),
    boot_id          TEXT,
    config_hash      TEXT CHECK (config_hash IS NULL OR (length(config_hash) = 64 AND config_hash NOT GLOB '*[^0-9a-f]*')),
    route_state_hash TEXT CHECK (route_state_hash IS NULL OR (length(route_state_hash) = 64 AND route_state_hash NOT GLOB '*[^0-9a-f]*')),
    nonce_issued_at  INTEGER,                       -- the hub's own clock at nonce issue: freshness is measured from here
    accepted_at      INTEGER,
    version          INTEGER NOT NULL DEFAULT 1,
    CHECK ((report IS NULL) = (generation IS NULL) AND (report IS NULL) = (nonce_issued_at IS NULL)
           AND (report IS NULL) = (fingerprint IS NULL) AND (report IS NULL) = (boot_id IS NULL)
           AND (report IS NULL) = (config_hash IS NULL) AND (report IS NULL) = (route_state_hash IS NULL)
           AND (report IS NULL) = (accepted_at IS NULL))
) STRICT;

-- The highest accepted identity per host and key (rollback and repeat detection). Written in the acceptance transaction.
CREATE TABLE helper_high_water (
    host_id          TEXT NOT NULL,
    fingerprint      TEXT NOT NULL CHECK (length(fingerprint) = 64 AND fingerprint NOT GLOB '*[^0-9a-f]*'),
    generation       INTEGER NOT NULL CHECK (generation >= 1),
    config_hash      TEXT NOT NULL CHECK (length(config_hash) = 64 AND config_hash NOT GLOB '*[^0-9a-f]*'),
    route_state_hash TEXT NOT NULL CHECK (length(route_state_hash) = 64 AND route_state_hash NOT GLOB '*[^0-9a-f]*'),
    updated_at       INTEGER NOT NULL,
    PRIMARY KEY (host_id, fingerprint)
) STRICT;
```

Nonces stay **in memory** (a restart only forces a re-request). The stored `report` is the received bytes (at most
64 KiB), kept so the Helper tab can show the verified set; it contains no secret. The applier guards: `Accept`
re-evaluates steps 8 to 10 inside the transaction against the stored high-water row and answers
`CommitError::Conflict` if it moved, so two concurrent verifications of one host cannot both advance the mark
(the service then asks the helper again with a fresh nonce; a consumed nonce is never reused);
`Pin` deletes the `(host, fingerprint)` high-water row, clears the verified columns and writes the audit row in the
same commit; `Unpin` deletes the pin and clears the verified columns, keeps high-water; host removal deletes the
three tables' rows for the host. Every write also writes its audit row (08, 09) in the same transaction.
`AggregateKind::HelperTrust` (`as_str` `"helper_trust"`) and the matching `StateWrite::HelperTrust(HelperTrustWrite
{ Accept, RecordOutcome, Pin, Unpin })` are added beside the variants 26, 34 and 42 added; fix every exhaustive
`match` (the in-memory twin and the crash harness). Retention: no purge; the rows are small and owner-owned.

## 9. Protocol and compatibility

- **Wire 1.2 is not bumped** (decided: unreleased; no 1.3). The kinds `attest_safety`, `safety_attestation`,
  `error`, `hello`, `agent_status` are unchanged; only the `ExpectedSafety` payload types are tightened.
  `hello.helper_identity` and `agent_status.helper_identity` start being populated.
- **Frozen frames.** Never edit `tests/protocol/fixtures/1_1/*` (`golden_frames_stay_at_1_1`). The 1.2 frames are
  `insta` snapshots: **no existing snapshot changes** (`run_capability.snap` keeps `"expected_safety": null`,
  `attest_safety.snap` and `safety_attestation.snap` keep their fields). Add two samples and their snapshots:
  `run_capability_contain` with `Verified{..}` and `run_capability_contain_unverified` with `Unverified{ reason:
  key_not_pinned }`, to freeze the strict wire form. Review each. If an existing snapshot changes, stop.
- **Older agents** answer `attest_safety` with `unsupported`; the hub maps it to detail `agent_unsupported` and
  gates `attestation_missing`. Older hubs never send it.
- **Hub downgrade:** tables only; an older hub ignores them and has no coverage (`UnavailableCoverage`).
- **API:** new endpoints only; OpenAPI and `schema.d.ts` change (section 15).

## 10. Error handling and recovery

**Request level (hub, nothing written):**

| Situation | Behaviour |
|---|---|
| `PUT` key while the hub holds no key from the host | `409 no_helper_key` |
| typed fingerprint is not 64 hex after removing spaces, or differs from the hub-computed fingerprint of the held key | `422 fingerprint_mismatch`; the key id is never accepted in its place |
| missing or wrong password, or the limiter trips | the existing step-up refusals (`401`, `429`); nothing pinned |
| re-check under 10 s after the last request for the host | `429 recheck_too_soon` |
| host unknown or offline | the existing host errors; a recheck on an offline host is refused, not queued |

**Attestation level:**

| Situation | Outcome |
|---|---|
| link timeout, not connected, session changed | no outcome change, nothing revoked; the previous state ages out by `FRESHNESS` |
| agent `error` reply with a token | `attestation_unsigned:<token>`; gate gives `attestation_missing`; latest outcome unchanged |
| pinned key differs, signature invalid, parse or hash failure | rejected with the wire reason; only a key mismatch changes the latest outcome (to mismatch) and revokes |
| signed incomplete from the pinned key | revoke, record token, latest outcome `signed_incomplete` |
| regression or repeat with different hashes | revoke, latest outcome `regressed`, audit `helper.generation_regressed` |
| next verified attestation after any of these | latest outcome verified; audit `helper.attestation_restored`; the high-water mark rules still apply |
| hub restart | pins, verified state and high-water persist; nonces are lost (re-request); the first request after `hello` refreshes |

**Helper crash windows:** interrupted state write leaves the old file; interrupted rotation leaves the old pair or a
key/state mismatch (`state_key_mismatch`, fail closed, `recover` resolves); a crash between `init`'s key and state
leaves a key without state: the key exists and `state` is absent, so the answer is the signed `state_missing`;
`recover` resolves it. The helper never retries a failed write on its own.

## 11. Security requirements

1. **Only the pinned key verifies.** A report is accepted only if its `public_key` equals the pinned 32 bytes and
   `verify_strict` over the received bytes succeeds, **before** any parsing. Parsing never re-serialises. Hashes are
   recomputed from the report's own lines.
2. **The hub computes the fingerprint.** Never from an agent field; the key id is a label (`ATTEST-3`).
3. **Nothing is pinned automatically**, including on first contact. Pin and unpin need session, CSRF, password
   and the full typed fingerprint. No chat, model, policy or background path pins.
4. **A verified identity is stored only from a complete, accepted report** (`ATTEST-2`); a signed incomplete report
   is never verified state (`ATTEST-4`); the high-water mark is written only in the acceptance transaction
   (`ATTEST-5`).
5. **Latest outcome, no sticky flag** (`ATTEST-7`); freshness only from the stored nonce issue time (`ATTEST-8`).
6. **Key custody.** The key and state are read and written only by helper code (`HELPER-2` of ADR 0030); the
   signing key type is constructed only in `keys.rs` and `lifecycle.rs`; rotation and recovery delete the old
   private key (`ATTEST-9`); no log, error, audit row or API response carries key bytes, report bytes beyond the
   stored verified report, a signature or a nonce.
7. **Strict wire types** (`ATTEST-10`); required `expected_safety` is enforced by one function (`ATTEST-6` lands
   with 46's `Contain` type; here the function and its test).
8. **Fail closed.** Every condition of 6.5.7 refuses; nothing resets automatically; an unsigned token unlocks nothing.
9. **Processes.** Route observation runs only through `ProcessExecutor` with argv vectors and an empty environment
   (R3, R4); no shell; fixed argv, the only variable being a validated IP address or a `u32` table.
10. **Dependencies.** `ed25519-dalek` and `rand` as `{ workspace = true }` in the helper crate; nothing else; rustls
   only, no OpenSSL (R11). No `unsafe`.

**Architecture rules (`xtask/src/arch/rules.rs`; a rules change needs a new ADR in `docs/adr`, checked by `arch-check
--base`, `xtask/src/arch/git.rs:83`).** Add: `helper_trust` to `UNZONED_MODULES` (`:515`); `attest_safety` to
`RAW_OPS` (`:203`) with the same file-allowed card as `send_transfer` (the link implementation and
`helper_trust/request.rs` only); `ZONES` unchanged. Gate tests (hub `tests/gates.rs`, core `tests/gates.rs`;
numbering and meaning from ADR 0032):

| ID | Rule |
|---|---|
| `ATTEST-1` | the verify call is made in one hub module (`helper_trust/verify.rs`) and only through a function that takes the pin; no other file mentions `verify_strict` for a safety report |
| `ATTEST-2` | `HelperTrustWrite::Accept` is built only in `helper_trust/accept.rs`, from a value only the complete-report path produces |
| `ATTEST-3` | the hub never builds a helper fingerprint from an agent-supplied field (`HelperIdentity` and `SafetyAttestation` have no fingerprint field; the hub reads none) |
| `ATTEST-4` | the verifier rejects an empty reason on an incomplete report, a non-empty one on a complete report, and a non-minimal incomplete shape |
| `ATTEST-5` | `helper_high_water` is written only by the `Accept` and `Pin` arms of the applier and is the only place the highest generation is read |
| `ATTEST-6` | the helper's request decoder refuses a `Contain` request without a valid `expected_safety` (the function and its test here; the scan over `Contain` types lands with 46) |
| `ATTEST-7` | no per-host mismatch set or flag exists in `helper_trust/` or the store; the gate reads `latest_outcome` |
| `ATTEST-8` | `gate` reads `nonce_issued_at` and nothing named `received_at` or `accepted_at` |
| `ATTEST-9` | rotation and recovery delete the old private key (a test over a fake directory tree, including `retired/`) |
| `ATTEST-10` | `ExpectedSafety` and the unverified reason refuse extra fields, unknown tags and unknown reasons; the reason is a closed enum |

The ADR (`docs/adr/0035-helper-attestation-boundary.md`, expected number, next free number at implementation time) records the `RAW_OPS` and zone edits and
the rules above. Also edit ADR 0032: record E1 (no bump, folded into 1.2), E2 (`helper_state_permissions`), E3
(non-UTF-8 names are `route_unrepresentable`), E4 (cap is `MAX_BASE64_BYTES`) and the E5 default of 6.5.6 as taken,
without changing its status (the owner accepts R-10, not this change).

## 12. Testing requirements

Fakes (existing unless marked): `FakeClock`, `FakeAgentLink` (extend: scripted `attest_safety` reply, an `error`
reply, timeout), in-memory repos and `uow_apply` twin (extend), `FakeCoverage` (override
`confirmation_current`), a fake `ProcessExecutor` returning fixture JSON, `HostFs` fixtures (**new**: fake helper
directory with modes, owners, a settable `boot_id`), and a **test-only signing seed** published in the test crate.
Tests never sleep; they advance the fake clock. Hub tests drive the router in-process like
`crates/domovoi-hub/tests/hub/packages.rs`; SQLite tests live in `crates/domovoi-store/tests/sqlite/`.

### 12.1 Where tests run

Core, proto, config, engine, store and hub tests run **natively on Windows and on Linux** (vectors are LF
fixtures, 12.4). Helper tests that need Unix modes, ownership, `flock` between processes or `SIGKILL` are
`#![cfg(unix)]` and run in WSL (`wsl.exe -d Ubuntu -- ...`, `TESTING.md`); they silently do nothing on Windows, so
list them in the completion report as "ran in WSL" with counts. The privileged custody test (12.5) never runs by
default.

### 12.2 Every RW-37 test, by name, with its owner

Each of the 76 names in RW-37 "Tests" has exactly one owner here. 47c-owned tests live in the crate shown and
keep the RW-37 name. "Pure" means they test the pure function the real code calls, not a permit or an nft apply
(neither exists yet).

| Group and names | Owner and where | How it runs |
|---|---|---|
| **Key exchange and enrolment**: `helper_public_key_exchange_and_fingerprint`, `key_id_never_used_for_verification`, `first_contact_does_not_pin_key`, `key_substitution_by_agent_is_detected`, `key_rotation_blocks_until_reenrolled`, `unpin_blocks_containment` (gate returns `key_not_pinned`; the actual block is 46's), `agent_reenrollment_keeps_helper_pin` | 47c, `crates/domovoi-hub/tests/hub/helper_trust*.rs` | native |
| **Signature and parsing**: `helper_attestation_signature_verifies`, `attestation_rejected_for_wrong_key`, `attestation_rejected_for_other_host` (nonce host binding), `helper_attestation_rejects_forged_report`, `helper_attestation_rejects_replayed_nonce`, `report_parser_rejects_duplicates_unsorted_and_noncanonical` | 47c, hub tests; the parser test in `crates/domovoi-core/tests/attest/` | native |
| **Agent trust**: `agent_cannot_fabricate_effective_safety_set` (an `agent_status` claiming interfaces or routes never yields `Covered`), `hub_rejects_unverified_safety_report` | 47c, hub and engine tests | native |
| **Confirmation and coverage**: `owner_cannot_confirm_uncovered_management_path`, `new_management_path_requires_local_helper_configuration`, `changed_helper_safety_set_invalidates_confirmation`, `stale_attestation_blocks_unattended_containment` (gate gives `attestation_stale`) | 47c, `crates/domovoi-engine/tests/services/` over `AttestedCoverage` and core `confirmation_current` | native |
| **Route state**: `nondefault_route_change_for_endpoint_changes_generation`, `irrelevant_route_change_keeps_generation`, `route_replacement_and_deletion_detected`, `multiple_tables_and_rules_observed`, `multiple_interfaces_and_gateways_observed`, `vpn_tunnel_route_to_hub_observed`, `incomplete_route_data_fails_closed` | 47c, `<helper>/tests/` over a fixture `RouteSource` and the captured `ip -j` fixtures | native |
| **Races**: `config_change_between_attestation_and_apply_refused`, `network_change_between_attestation_and_apply_refused`, `reboot_between_permit_and_apply_refused`, `generation_change_between_permit_and_apply_refused` | 47c, **pure**: `check_expected` and `permit_identity_current` over a `SafetyIdentity`. The integration with a real `PolicyPermit` is 45's and 46's (they add `permit_identity_binds_boot_id`) | native |
| **Helper enforcement**: `helper_fails_closed_when_safety_state_unknown`, `helper_attestation_key_not_readable_by_agent_or_skill` | 47c; the second is **harness-excluded** and privileged (12.5) | native; privileged opt-in |
| `helper_refuses_target_overlapping_exclusion_even_if_agent_requests`, `helper_refuses_overlap_without_expected_safety` | **46** (target canonicalisation and the effective exclusion set exist only with the `Contain` apply). 47c owns the half that does not need a target: `helper_refuses_missing_expected_safety` | 46 |
| **Unverified human path**: `malformed_expected_safety_refused`, `helper_refuses_missing_expected_safety` | 47c (core, proto and `check_expected`) | native |
| `human_containment_with_no_pinned_key_uses_unverified_path`, `human_containment_with_expired_attestation_uses_unverified_path`, `human_containment_with_invalid_signature_uses_unverified_path`, `human_containment_with_incomplete_routes_uses_unverified_path`, `unattended_containment_refused_in_every_unverified_case`, `agent_refuses_unverified_with_policy_label`, `exclusion_overlap_refused_even_on_unverified_human_path`, `no_management_cidrs_refuses_containment` | **46** (they need the `Contain` request, the approval surface and the apply). 47c ships the pure inputs they call and tests them under its own names: `decide_gives_a_reason_for_every_unverified_case`, `decide_refuses_policy_authority_in_every_unverified_case`, `agent_accepts_refuses_unverified_for_policy_authority` | 46 |
| `human_approved_containment_shows_uncovered_paths` | **46** (the card). 47c: `unprotected_paths_lists_every_uncovered_candidate` | 46 |
| **Helper state**: `generation_state_missing_with_key_fails_closed`, `generation_state_corrupt_with_key_fails_closed`, `generation_state_key_mismatch_fails_closed`, `generation_never_decreases_under_same_key`, `concurrent_attestations_serialize_generation_updates` (`flock`, child processes), `interrupted_state_write_never_reuses_generation`, `helper_state_recovery_requires_explicit_local_action`, `key_rotation_invalidates_previous_pin`, `first_attestation_is_generation_one_after_init`, `containment_membership_change_keeps_generation` (the generation function takes the hashes only; the nft-level variant is 46's) | 47c, `<helper>/tests/` | native; `flock` and kill tests `cfg(unix)`, WSL |
| `helper_fails_closed_on_wrong_directory_mode` | 47c for the `safety.attest` path (`Unsigned(helper_state_permissions)`); 47a owns the start-check variant. If 47a already has a test of this name, extend it instead of adding a second | `cfg(unix)`, WSL |
| **Hub state**: `hub_rejects_generation_regression`, `repeated_generation_with_different_hashes_rejected`, `high_water_persisted_across_hub_restart`, `incomplete_reason_empty_only_when_complete`, `incomplete_report_has_minimal_shape`, `signed_incomplete_revokes_verified_state`, `mismatch_cleared_by_next_verified_attestation`, `freshness_measured_from_nonce_issue_time` | 47c, hub, store and core tests | native |
| **Encoding (04b vectors)**: `config_hash_matches_vectors`, `route_state_hash_matches_vectors`, `equivalent_toml_same_config_hash`, `report_rejects_invalid_encoding_vectors` | 47c, `crates/domovoi-core/tests/attest/` (C1 to C3 TOML inputs go through `domovoi-config`: put the C1 to C3 TOML cases in a config test and assert the same digests) | native |
| **Freshness**: `permit_from_before_reboot_refused` (pure: `permit_identity_current` with a differing `boot_id`), `confirmation_survives_reboot_only_when_identity_otherwise_equal` | 47c, core tests | native |
| **Limits**: `agent_limit_stricter_than_helper_applies`, `agent_limit_above_helper_is_refused` (the rename of `agent_limit_above_helper_is_capped`: the helper refuses an over-maximum TTL, it does not clamp), `ttl_above_maximum_refused_not_clamped`, `helper_active_count_is_authoritative` | **46** (containment apply: TTL, `max_active`, the nft set count) | 46 |
| `mutation_limit_survives_helper_restart` | **45** (the first helper write group owns the persisted limiter of RW-39 `[limits] mutations_per_hour`); 46 reuses it | 45 |
| **Unrelated work**: `monitoring_unaffected_by_missing_management_paths` | 47c, **harness-excluded**: a hub and engine test with the real `AttestedCoverage` over a view that is always unverifiable, asserting that evaluation, alerts and human-approved non-containment work continue while confirmation is refused | native, in CI |

Count: 76 names. 47c owns 60 in full, 46 owns 15 and 45 owns 1, for the reasons above. For the unverified-path and
approval-card names that 46 owns, 47c ships and tests the pure inputs under its own names (listed in the table).
Nothing is dropped. 46 lists its 15 names in its section 12.1 and 45 its one in its section 12.2, with the same counts (60 + 15 + 1 = 76).

### 12.3 Tests required (named after behaviour; all are in addition to 12.2)

**Core `attest`** (`crates/domovoi-core/tests/attest/`): `c_vectors_match_published_digests` (C1 to C4, lengths and
digests of 6.3), `r_vectors_match_published_digests` (R1 to R4), `c1_and_c2_are_the_same_effective_configuration`,
`selector_encoding_is_injective_for_names_with_separators`, `r4_digest_unchanged_by_inner_escaping`,
`oif_selector_escaping_matches_iif`, `ipv6_corner_forms_are_canonical`, `every_vector_line_passes_the_strict_line_grammar`,
`invalid_utf8_is_rejected_as_its_own_step`, `complete_report_with_zero_hash_rejected`, `complete_report_with_generation_zero_rejected`,
`incomplete_report_with_repeated_lines_rejected`, `oversize_report_is_refused`, `fingerprint_rejects_uppercase_and_wrong_length`,
`boot_id_rejects_uppercase_hyphenless_and_braced`, `safety_identity_canonical_round_trips`, `safety_identity_parse_is_strict`,
`bare_report_signature_does_not_verify`, `signature_vector_is_deterministic_for_the_test_seed`,
`unit_names_with_backslash_are_refused_as_config_invalid`, `max_ttl_zero_is_config_invalid`, `unknown_safety_key_is_config_invalid`,
`mandatory_units_cannot_be_removed`, `computed_at_not_used_for_trust`.

**Proto**: `expected_safety_refuses_extra_field`, `expected_safety_reason_is_closed_enum`,
`expected_safety_unknown_tag_refused`, `unverified_serialises_to_state_and_reason_only`,
`max_report_bytes_equals_base64_cap`, `hash32_and_sha256bytes_encode_identically`, `hash32_refuses_uppercase_hex`, `existing_1_2_snapshots_unchanged`, the two new snapshots, and the rewritten
`expected_safety_tagged_variants_roundtrip` and `malformed_expected_safety_refused` of `tests/protocol/extensions.rs`.

**Helper** (`<helper>/tests/`): `init_writes_generation_zero_and_zero_hashes`, `init_refuses_existing_key_or_state`,
`rotate_key_deletes_old_private_key` (`ATTEST-9`), `recover_always_creates_new_key`,
`retired_directory_keeps_public_key_and_state_only`, `fingerprint_command_matches_hub_computation`,
`lock_open_error_has_its_own_token`, `state_file_has_exact_format_checksum_and_modes`,
`leftover_state_tmp_is_deleted_under_the_lock`, `state_is_durable_before_the_report_is_signed` (the stage hook
orders write, file sync, rename, directory sync, then sign), `state_write_failed_is_signed_incomplete_without_generation_change`,
`oversize_report_fails_closed`, `blackhole_rule_or_route_is_incomplete_not_hashed`,
`non_utf8_interface_name_is_route_unrepresentable`, `multipath_route_is_route_unrepresentable`,
`ip_json_parsers_match_captured_fixtures`, `unknown_rule_key_is_unrepresentable_not_ignored`,
`truncated_ip_output_is_a_failed_read`, `ip_is_run_with_argv_and_a_fixed_environment`,
`hub_url_literal_ip_becomes_hub_addr`, `hub_url_hostname_yields_no_hub_addr_line`,
`link_peer_lines_are_empty_in_this_release`, `unsigned_tokens_never_carry_a_signature`,
`attest_without_group_safety_attest_is_refused`.

**Agent**: `attest_safety_relays_report_and_signature_bytes_unchanged`, `attest_safety_with_unsigned_token_replies_error_with_that_code`,
`attest_safety_without_helper_replies_helper_unavailable`, `agent_announces_helper_public_key_and_no_fingerprint`.

**Hub and store**: `received_key_from_hello_is_stored_and_fingerprinted_by_the_hub`,
`hub_ignores_any_agent_supplied_fingerprint`, `pin_requires_password_and_full_typed_fingerprint`,
`pin_with_key_id_only_is_refused`, `pin_audits_key_pinned_then_key_rotated_with_old_and_new_fingerprints`,
`owner_repin_resets_high_water_for_that_key`, `rotation_starts_a_new_high_water_row`,
`same_key_repin_after_mismatch_unblocks`, `nonce_is_consumed_only_after_signature_parse_and_hash_checks`,
`forged_reply_does_not_burn_the_nonce`, `nonce_boundaries_are_strict_at_60_and_300_seconds`,
`unsigned_token_gates_attestation_missing`, `error_frame_reply_does_not_wait_for_timeout`,
`link_timeout_revokes_nothing`, `recheck_is_rate_limited`, `high_water_written_in_the_acceptance_transaction`,
`concurrent_acceptance_cannot_advance_the_mark_twice`, `host_removal_deletes_pin_state_and_high_water`,
`step_up_registry_lists_helper_pin_and_unpin_as_enforced`, `no_audit_row_carries_report_signature_or_nonce`,
`helper_dto_is_hub_computed_and_omits_verified_when_the_gate_fails`.

**Engine**: `attested_coverage_returns_covered_uncovered_and_unverifiable`,
`confirmation_current_follows_the_reboot_exception_only`, `unavailable_coverage_still_refuses_every_confirmation`.

**Web** (`web/src/features/capabilities/`): `helper_panel_shows_each_host_state_and_reason`,
`helper_panel_never_claims_verified_without_a_verified_report`, `helper_key_page_pin_needs_full_fingerprint_and_password`,
`helper_key_changed_banner_is_always_visible`, `helper_views_use_only_helper_endpoints`, plus the axe check that every
page test performs and the visibility gate for the two new P0 ids.

**Gate tests**: `ATTEST-1` to `ATTEST-10` as in section 11.

**Property tests** (`proptest` is a workspace dependency; no new dependency, no `fuzz/` crate: a fuzz crate would
trip "every workspace member in `CRATES`"): `parse_report_accepts_only_what_it_renders`
(for any bytes, accept implies `render(parse(x)) == x`), `single_byte_mutation_of_a_valid_report_is_rejected_or_rerenders_identically`,
`field_codec_round_trips_every_byte_string`, `encode_of_decode_is_identity_for_accepted_text`,
`selector_parse_render_round_trip_for_arbitrary_interface_names`, `state_parser_round_trips`,
`expected_safety_parser_round_trips`, `generation_sequence_is_non_decreasing_under_interleaved_changes_and_restarts`
(no repeats while state is not restored). Keep the case count modest so the suite stays fast.

### 12.4 Fixtures and vectors

- Copy the byte-exact vector files named in 6.3 into `crates/domovoi-core/tests/attest/vectors/` and add a
  `.gitattributes` rule marking `crates/domovoi-core/tests/attest/vectors/**` and the `ip` fixture directory
  `-text` (or `eol=lf`): the digests are over LF bytes and break under a CRLF checkout on Windows. A test asserts no
  fixture contains a CR.
- **Committed-file rule.** The 04b vectors use private-range addresses and example service names, and the committed
  repository forbids LAN addresses and container names in committed files. The vectors are the owner-accepted
  contract and their digests depend on the exact bytes, so the default is to commit them **verbatim** (they are
  synthetic examples, not the owner's network) and to list this as a deviation to confirm in the completion report.
  If the owner rules that even synthetic text must go, apply the neutral-twin procedure instead: rewrite every
  input with documentation-range addresses (`192.0.2.0/24`, `198.51.100.0/24`, `2001:db8::/32`) and neutral names
  (`svc-a`, `ctr-a`), compute the new digests with the **research harness** edited the same way (run its
  `tests/encoding_vectors.rs` against the rewritten inputs) and with the production code, require both to agree,
  and freeze those. All **new** fixtures (the `ip -j` captures, state files, reports) use documentation ranges and
  neutral names from the start.
- Semantic vectors (a rule and route snapshot as input, the expected route lines as output) replace reliance on R1
  for the selection logic; add at least: two tables with a fwmark rule, a VPN default route, a gateway change, a
  route replaced and deleted, an irrelevant route change, and an unrepresentable rule.

### 12.5 The two harness-excluded tests, and the live-host tests

- `helper_attestation_key_not_readable_by_agent_or_skill` (the harness could only adapt it): a privileged test in
  `<helper>/tests/privileged_custody.rs`, `#[ignore]`d and also gated by `DOMOVOI_PRIVILEGED_TESTS=1`. It needs
  root and `useradd`: run it **only on a disposable host**, with `sudo cargo test -p <helper-crate> --test
  privileged_custody -- --ignored`. It starts the **real** helper under the real accounts 47a's installer creates
  (or two disposable accounts it creates and removes), and repeats the real-user probe set of the research
  (`stat`, `open`, copy, create, rename, link, `chmod`, `unlink`, `flock`, `/proc/<pid>` `environ`/`fd`/`mem`/`maps`,
  `ptrace`, `process_vm_readv`) as both non-root users against the key, state, lock and the running helper: all
  refused. `domovoi helper selftest` ST-15 (47a) runs the file-level half on every install. A hosted Linux runner
  with `sudo` may run it as a manual-dispatch job (unverified; report the result either way).
- `monitoring_unaffected_by_missing_management_paths`: runs in CI as described in 12.2.
- Live route observation (policy rules, a tunnel, several tables), release-build cost with `ip` process starts and
  the socket round trip, power-loss durability and `ENOSPC` of the state write, and the real `boot_id` are
  **host-only**, not CI. Do the live read-only observation on the dev host in step 1 and report it; do not claim
  more.

## 13. Acceptance criteria

1. The eight vectors reproduce byte for byte through core and config; the escaping vectors and `r4_digest_unchanged_by_inner_escaping` pass; fixtures are LF and frozen.
2. The strict parser refuses every case of 6.1 and satisfies the round-trip property; an incomplete report has only the minimal shape; a complete one never has a zero hash or generation 0.
3. `safety.attest` returns a signed report whose signature verifies under `verify_strict` with `SIGNING_PREFIX`, or the closed unsigned token for each condition of 6.5.7; no condition resets state or creates a key.
4. `init` writes generation 0 with zero hashes; the first attestation is generation 1; `rotate-key` and `recover` produce a new key and sequence and leave no copy of the old private key anywhere in the tree.
5. State writes are durable (file sync, rename, directory sync) before signing, inside the lock; concurrent attests never duplicate a generation; an interrupted write never reuses one.
6. Route observation uses only `ip -j` argv through `ProcessExecutor` with an empty environment; a non-lookup rule, a non-unicast or multipath route and a non-UTF-8 name give `route_unrepresentable`, never an omitted item.
7. The hub never pins automatically; pin needs session, CSRF, password and the full typed fingerprint compared with the hub-computed one; the key id is never accepted; unpin and host removal block coverage.
8. Verification follows the table of 6.7.3 in order; the signature is checked over the received bytes before any parse; the nonce is consumed only after steps 1 to 5; freshness is measured from the stored nonce issue time with strict boundaries.
9. The gate follows the latest-outcome rule; mismatch, signed incomplete and regression revoke; one good attestation restores; no sticky flag exists.
10. The high-water mark per `(host, fingerprint)` rejects a lower generation and an equal generation with different hashes, persists across a hub restart, is written in the acceptance transaction, and is reset only by an owner re-pin of that key.
11. `ExpectedSafety` is strict (extra field, unknown tag, unknown reason, malformed hash or `boot_id` refused), lives in core and is re-exported by proto; no existing 1.2 snapshot and no `1_1` fixture changes; two new snapshots exist.
12. `UnverifiableReason` has `PathUncovered` and `ConfirmationStale`; `AttestedCoverage` replaces `UnavailableCoverage` in the hub wiring; `CoverageEvidence` carries the canonical identity; the reboot exception applies to confirmations only.
13. The agent relays report and signature bytes unchanged and answers an unsigned token with an `error` frame; the hub does not wait for a timeout.
14. The four endpoints exist with the stated verbs, codes and DTO; OpenAPI and `schema.d.ts` are regenerated and committed.
15. The placeholders are gone, the real Helper tab and key page ship under `web/DESIGN.md` rules, the replaced gate test passes, and every file is at most 250 lines.
16. Every RW-37 test has its owner per 12.2; the 60 owned names plus the new tests exist and pass; the two excluded tests run as described; the privileged test is `#[ignore]` by default.
17. `ATTEST-1` to `ATTEST-10` pass; `cargo xtask arch-check` and `--base` pass with the ADR; no new crate, no new dependency beyond the two named, no new `#[allow]`, `unwrap`, `expect` or `panic!` in library code; non-test files at most 500 lines.
18. Audit rows exist for every transition of 6.7.5 and none carries key, report, signature or nonce bytes.

## 14. Migration and compatibility

Existing installs: nothing changes until a helper exists and the owner pins its key; the new tables stay empty,
`AttestedCoverage` answers `key_not_pinned` and no path can be confirmed, which is exactly today's behaviour
(`UnavailableCoverage` refused everything). Existing agents without the relay answer `unsupported` and gate
`attestation_missing`. Existing config: `[safety]` is already in the helper's documented `helper.toml`; an
installation with no `[safety]` has an empty one. Backups: the database backup holds pins, verified state and
high-water marks; a restore of the **hub** database also restores an older high-water mark (a stated residual,
ADR 0032 "What the checker cannot see"). Hub downgrade: tables only. Prompts 45 and 46 consume `SafetyIdentity`,
`gate`, `confirmation_current`, `permit_identity_current`, `decide`, `agent_accepts`, `require_expected_safety` and
`HelperTrust::fresh` without changing what is specified here.

## 15. Documentation requirements

- `docs/rework/04b-attestation-encoding.md`: record that the vectors are frozen as fixtures and where; add the
  semantic vectors and the new vectors list; note the E5 default.
- `docs/rework/03-architecture-decisions.md` RW-37: replace "via netlink" with `ip -j` through `ProcessExecutor`;
  record the E5 default, the unsigned token `helper_state_permissions` and `helper_unavailable`, and that two tests
  were harness-excluded and are now owned as in 12.2. `04` §8.3 and §9 only where they restate the same facts.
- `docs/rework/99-open-decisions.md`: no status change; list the E5 default under R-10 as taken and awaiting
  confirmation. `docs/rework/07-implementation-prompt-index.md`: mark 47c written.
- `docs/adr/0032-helper-safety-attestation.md`: E1 to E5 as in section 11; the new ADR of section 11.
- OpenAPI: `cargo xtask openapi && cd web && npm run gen:api`; commit both generated files, never hand-edit them.
- `docs/book/src/reference/protocol.md`: the attestation exchange (nonce, relay, error reply), the strict
  `expected_safety` form and that `hello.helper_identity` is now populated. `docs/book/src/security.md`: the helper
  key, the pin, the fingerprint comparison, why the agent cannot forge a report, what "unverified" means.
  `docs/book/src/reference/hub.md`: the helper endpoints, the Helper tab, re-check and the 15-minute cadence.
  `docs/book/src/reference/agent.md` or the helper page 47a created: `domovoi helper init`, `rotate-key`, `recover`,
  `fingerprint`, the state files and the fail-closed tokens with their local actions; the sentence about
  covering the hub's address when the hub URL is a name. `docs/book/src/guide/` helper page (new, add to
  `SUMMARY.md`): enrolling a helper, rotating its key, what each reason means. Run `cargo xtask docs-links`.
- `ARCHITECTURE.md`: §5.9 (core `attest`), §6 ports (`HelperTrustRead`, the `ManagementCoverage` row now
  attestation-backed), §7 hub (`helper_trust`), §8 (the attestation exchange), §9 (the three tables), §15
  invariants (the helper key is read only by helper code; only a pinned key verifies; the hub never pins
  automatically; trust follows the latest outcome); §4 only if a dependency edge changed (it must not).
- `web/DESIGN.md`: remove the "Planned" text for the helper views; document the new statuses.
- Commits, book pages and the PR must not mention prompts, prompt numbers, the build plan or coding agents.

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
