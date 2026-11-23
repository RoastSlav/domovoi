# Implementation prompts

One file per prompt: `NN-branch-name.md`. `NN` is the stable ID used in `../07-implementation-prompt-index.md`.
Each file is a self-contained, copy-paste-ready brief for one branch and one pull request.

**Every prompt in the index has a file.** The helper, runner and autonomy briefs 44, 47a, 47b, 47c, 47d, 45 and 46 are
written; their launch order, gates and cross-brief names are below. Prompt 99, the closing cleanup, runs last, after everything else has merged. `../07-implementation-prompt-index.md` is authoritative for prompt status,
dependencies, launch order, gates and the integration branch (its §6); an index row without a file
here is a plan, not a brief, and must not be launched.

- **One at a time.** Prompts are launched manually, one branch and one pull request at a time. `∥` in the
  index means the prompts have no dependency forcing an order — it never means concurrent execution.
- **Migrations are sequential.** A prompt that adds a migration runs only after the previous
  migration-adding prompt has merged, and takes the next free number (index §7).
- **One active V2 branch.** `overhaul-investigation` is the base for all V2 work and holds both the
  internal planning documents and the V2 implementation. Branch from its latest HEAD and open the pull
  request against it; `main` is never a prompt target (index §1.1). The coding agent works in the
  repository checkout and reads `docs/rework/`, `docs/design-pack/` and these prompt files there — no
  second branch, worktree, documentation overlay or sync step is needed.
- **Internal documents stay out of `main`.** `docs/rework/`, `docs/design-pack/` and these prompts are
  never merged into `main`; product documentation under `docs/book/` travels with the feature that
  changes it. Promotion to `main` is a curated branch later, not a merge of the whole V2 branch (index
  §1.2).
- **Authoritative design:** `../03-architecture-decisions.md` (RW-NN), `../04-target-architecture.md`,
  `../04b-attestation-encoding.md`, `../99-open-decisions.md` (D-NN, O-NN, R-NN).
- **Linux research environment:** the owner's Ubuntu WSL2 environment, recorded in
  [`../investigations/H-wsl2-research-environment.md`](../investigations/H-wsl2-research-environment.md).
  Research prompts 53, 54 and 56 may run there for scoped, preliminary evidence; prompt 55 needs no
  environment gate. WSL2 evidence does not accept a research item and does not stand for supported
  conventional Linux hosts.
- **Repository rules:** `AGENTS.md`, `ARCHITECTURE.md`, `TESTING.md`.

The design pack in `docs/design-pack/` is product intent, not an implementation reference.

## Shared contracts and their owning prompts

Exactly one prompt introduces each shared contract. A later prompt consumes it. When a consuming prompt
runs, it must read the owning prompt's merged code and use the actual names. The names listed here are
the planned names.

| Contract | Owner | Planned names (verify in code; for 45 to 47d see "Cross-brief names") |
|---|---|---|
| Unit of work port, transaction, commit errors, journal and effects tables | 08 | `domovoi_core::ports::UnitOfWork`, `Transaction`, `StateWrite`, `JournalDraft`, `AuditDraft`, `EffectDraft`, `CommitError`, `CommitReceipt`; tables `journal`, `effects`, `schema_compat` |
| `Actor` and audit hash chain | 09 | `domovoi_core::Actor`; table `audit_anchor`; audit columns `prev_hash`, `hash` |
| Exact protocol rule, durable incompatibility | 13 | `agents.reported_protocol`, `reported_release`, `last_authenticated_at`, `incompatible_since` |
| Host availability | 14 | `HostAvailability { Known, Unknown, Absent, Incompatible }` |
| Effect lifecycle with lease fencing | 15 | `effects.claim_token`, `claim_generation`, `lease_until` |
| Wire 1.2: complete v1 contract (RW-42) | 20 | `hello` extensions, `agent_status`, `run_capability`, `refused`, `action_status`, `get_catalog`, `transfer_*`, `attest_safety`, `safety_attestation`, `unsupported`, `Open<T>` |
| `ExecutionId`, plan hash v2, `Authorization`, `PolicyPermit`, `KeyFingerprint` | 22 | `domovoi_core::action::{Authorization, PolicyPermit, PlanHash}`, `ExecutionId`, `domovoi_core::host::KeyFingerprint` |
| Capability contracts | 24 | `domovoi_core::capability::{ContractId, EffectClass, CapabilityContract}` |
| Capability registry, availability and target identity | 25 | `domovoi_core::capability::Availability`; ports `domovoi_core::ports::{CapabilityRegistry, TargetResolver}`; `TargetQuery`, `ResolvedTarget`, `CatalogueState`, `canonical_host_identity` |
| Operation aggregate and trace schema | 26 | `Operation`, `OperationStatus`, `Step`, `StepKind`, `StepOutcome`, `StepEdge`, `EdgeKind`; trace JSON golden fixtures |
| Journal-backed SSE | 28 | SSE `id` = journal `seq`; topics; `as_of_seq` |
| Context retrieval port | 31 | `domovoi_core::ports::ContextSource`, `domovoi_core::context::*`, `MetaTool`, `ToolSet` |
| Policy evaluator | 39 | `domovoi_engine::policy::{evaluate, evaluate_shadow, Decision, Rule, simulate, validate_publication}`; `EvaluateRequest`, `Outcome`, `PublicationReport`, `PolicyError` |
| Visibility categories | 37 | `useVisibility(category)`; categories `P0` to `P3` |
| Operation read API, parity suite | 27 | `GET /api/v1/operations`, `/{id}`, `/{id}/trace`, `/{id}/steps/{step}`, step approve/deny routes, `OperationPage`, `OperationDetailDto`, `ExecutionDto`; `tests/services/parity.rs` |
| Journal-backed SSE details | 28 | `GET /api/v1/events`, frames `ready`/`journal`/`reset`, `JournalFeed`, `topics_of`, `snapshot_seq`, `useJournalStream`, `journal_floor` |
| Hub use-case layer | 29 | `crate::services::{Services, Caller, OperationCommands, ...}`; arch rules `MOD-1`…`MOD-4`, `CALLER-1`; control routes `/cancel`, `/pause`, `/resume`, `/steps/{step}/answer` |
| Right slot and approval card | 30 | `web/src/app/rightSlot/` (`RightSlotProvider`, `RightSlotHost`, `RightSlotSurface`, `useRightSlot`), kit `SidePanel`, `ApprovalCard`, smoke mock `npm run smoke` |
| Context registration | 32 | `TriageDeps.extra_context: Vec<Arc<dyn ContextSource>>`, `run_chain` |
| Knowledge store | 34 | `domovoi_core::knowledge::*`, `KnowledgeRead`, `ManagementCoverage`, `KnowledgeSource`, `FactRecordId`, `/api/v1/knowledge/*` (`KnowledgeFactDto`, ...) |
| Experience | 35 | `MetaTool::SearchExperience`, `EffectKind::DraftResolution`, `/api/v1/experience/search`, similar-operation routes |
| Conversations | 36 | `ConversationService`, `AnswerSink`, SSE `live.start`/`live.text`/`live.tool`/`live.end`, `/api/v1/conversations` |
| Policy store | 40 | `StateWrite::Policy`, `PolicyRepo`, `ShadowRecorder`, `/api/v1/policy/*` |
| Packages, agent side | 41 | `domovoi_core::package::*`, `package_contracts()`, `PackageFailureCode`, testkit `PackageFixture` |
| Packages, hub side | 42 | `domovoi_core::install::*`, `PackagePlan`, `/api/v1/packages/*`, `/api/v1/hosts/{name}/packages` |
| Step chat approvals | 59 | `executions.chat_token_hash`, `OperationHandle::authorize(.., chat_token)`, `engine/src/chat_token.rs`, `EventKind::ExecutionChanged` |
| Accounts, helper skeleton and protocol, isolation checks | 47a | `domovoi_core::{identity, helper, isolation}`, `HostInspect`, crate `domovoi-helper`, `HelperRequest`, `HelperResponse`, `HelperClient` |
| Skill runner | 47b | `domovoi_core::skill_runner`, `domovoi skill-runner serve`, `domovoi-skill-runner.socket` |
| Helper safety attestation | 47c | `domovoi_core::attest::{SafetyIdentity, ExpectedSafety, Hash32}`, `helper_trust`, `AttestedCoverage` |
| Helper read operations | 47d | `domovoi_core::helper::read`, `DockerReadApi`, `PrivilegedReads` |
| Permit issuer and `unit.restart` | 45 | `PermitIssuer`, `POLICY_DISPATCHABLE`, `domovoi_core::unit` |
| Containment | 46 | `firewall.block_source`, `ContainmentPlan`, `containment_blocks` |

## Launch order of the helper, runner and autonomy briefs

One branch and one pull request at a time, in this order. The real dependency edges are the ones shown;
anything the index (§3 waves L and M) shows as a looser order is tightened to this one.

| Step | Brief | Needs merged first | Gate (owner decisions) | Migration | ADR (expected number) |
|---|---|---|---|---|---|
| 1 | 44 `helper-adrs` | 54 and 55 evidence | none | none | ADRs 0030 to 0032 exist as `proposed` in an open pull request; 44 counts as done when that request merges |
| 2 | 47a `helper-accounts-and-skeleton` | 44, 20, 41, 42 | R-3 and R-9 accepted **for building** (47a "Launch gate") | none | 0033 `helper-crate-and-isolation-rules` |
| 3 | 47b `skill-runner` | 47a | R-9 accepted for building | none | 0034 `skill-runner-process` |
| 4 | 47c `helper-attestation` | 47a (47b is order-flexible with it, never concurrent) | R-10 accepted; owner confirms the E5 default of 47c section 6.5.6 | first of the series, expected `0020` | 0035 `helper-attestation-boundary` |
| 5 | 47d `helper-read-operations` | 47a, 47b, 47c | R-3 accepted for building; owner confirms OD-1 (the widened helper unit baseline) | none | 0036 `helper-read-operations` |
| 6 | 45 `policy-permit-autonomy` | 47a to 47d, 40, 39 | R-8 accepted | expected `0021` | 0037 `permit-issuer-and-unit-restart` |
| 7 | 46 `containment` | 45, 47a to 47d | R-2 and R-10 accepted; the leak self-test returns `open` on one platform | expected `0022` | 0038 `containment-boundary` |

Notes that apply to the whole table:

- **Numbers are expectations, not contracts.** At implementation time each brief lists
  `crates/domovoi-store/migrations/` (it ends at `0019_install_jobs.sql` today) and `docs/adr/` on the branch it will land
  on and takes the next unused number (index §7). If another migration or ADR has merged in between, the later numbers
  move and the brief says so in its PR. ADRs 0030 to 0032 are in an open pull request; 0033 onward are the next free ones.
- **Every ADR is `proposed`.** Each of the six briefs changes `xtask/src/arch/rules.rs` and so needs its own numbered ADR
  (`cargo xtask arch-check --base origin/overhaul-investigation` fails without one). The test `rules_file_change_has_an_adr`
  is 47a's gate; the later briefs keep it green and add no copy.
- **Wire stays 1.2** (unreleased; the latest release carries 1.1). Nothing under `crates/domovoi-proto/` gets a version
  bump in this series; 47c changes the 1.2 types in place and regenerates the golden frames. `HELPER_PROTOCOL` (the helper
  socket's own number) stays `1` for the whole series because no release is cut from the integration branch between 47a and
  46; if one is, 47d bumps it (its section 9).
- **Required CI.** Required today: the existing jobs, including `units` and `installer`. New and **not** required until they
  have been green three times in a row and the owner agrees: `privileged` (47a; 47b, 47c and 47d add rows to
  `deploy/test/identities.sh` and the runner script it runs) and `containment-ns` (46). `containment-leak` (46) is never
  required. 47c adds no job of its own: its privileged test `privileged_custody.rs` runs in `privileged`.

## Cross-brief names

One table, written after reading the six briefs as a set. "Defined by" is the brief whose text is authoritative for the
spelling and signature; every other brief uses it as written. A merged code name that differs from this table wins and is
reported in the PR (each brief's step 1).

| Name | Kind | Defined by | Used by |
|---|---|---|---|
| `domovoi-helper` | crate (edges `core`, `config`) | 47a | 47b, 47c, 47d, 45, 46 |
| `domovoi_core::identity` (`AGENT_ACCOUNT`, `SKILL_ACCOUNT`, `expected_paths`, `evaluate_*`) | module | 47a | 47b |
| `domovoi_core::isolation` (`CheckId` ST-01 to ST-20, `IsolationReport`, `evaluate_isolation`, `BoundaryState`, `PlatformTier`, `WriteOptIn`, `TierDecision`, `classify_platform`) | module | 47a | 47b, 47d, 45, 46 |
| `HostInspect`, `FakeHostInspect`, `UnixHostInspect` | port, fake, adapter | 47a | 47b, 47d |
| `HelperRequest` (closed, `tag = "op"`) | enum | 47a: `Health`, `Groups`, `Read { group }`, `SafetyAttest { nonce }` | 47c (gives `SafetyAttest` behaviour), 47d (removes `Read`, adds nine typed read variants, `group()`), 45 (adds `UnitIdentity`, `UnitRestart`), 46 (adds the `firewall.*` variants) |
| `HelperResponse`, `UnavailableReason`, `RefusalToken`, `StartRefused` | enums | 47a | 47d (read variants, `ReadFailed`), 45 (`Outputs`, `UnitRefused`), 46 |
| `HelperGroup`, `ReadGroup`, `HelperConfig::effective_groups()` | enum, fn | 47a | 47d, 45, 46 |
| `Nonce` (64 lowercase hex, helper wire) and `Nonce32` (hub side) | types | `Nonce`: 47a; `Nonce32` and its conversion: 47c | 47d (`Nonce`) |
| `MAX_FRAME_BYTES = 65_536` of `domovoi_core::helper` | const | 47a | 47d renames it `MAX_REQUEST_BYTES` (same value) and adds `MAX_RESPONSE_BYTES = 4_194_304`; 47c's report cap `MAX_REPORT_BYTES` is 64 KiB, below it |
| `MAX_FRAME_BYTES = 16 MiB`, `MAX_REQUEST_BYTES = 64 KiB` of `domovoi_core::skill_runner` | consts, a different module | 47b | none (not the helper's constants) |
| `HELPER_PROTOCOL = 1` | const | 47a | 47d, 45, 46 (unchanged) |
| `HelperClient`, `probe_status`, `HelperStatus` | agent client | 47a (`helper_client.rs`) | 47d turns the file into the directory `helper_client/` (rule `HELPER-1` path changes), 47c, 45, 46 |
| `helper.toml`: `groups`, `[platform] allow_write_groups_on_untested`, `[smart] devices`, `[safety]`, `[safety.containment] max_ttl`/`max_active` (default `0`), `[limits] timeout`/`mutations_per_hour` | config keys | 47a (parsed) | 47d interprets `[docker] endpoint` and `[smart] devices`; 47c interprets `[safety]` and `max_active`; 45 interprets `[systemd]`, `protected_units` and `mutations_per_hour`; 46 interprets `[nft]` and `[safety.containment]` |
| `/var/lib/domovoi-helper/`: `state.lock`, `platform.json` | files | 47a | 47c, 45, 46 |
| `attest.key`, `state` (generation file) | files | 47c | 46 |
| `restarts` (persisted restart ring and write count) | file | 45 | 46 (shares the limiter) |
| `containment.journal`, `nft-selftest.json`, `drills.json` | files | 46 | none |
| `StateLock`, `write_atomic_locked`, `run_start_checks`, token `helper_state_permissions` | fns, token | 47a | 47c, 45, 46 |
| `domovoi helper serve`, `init`, `repair` | subcommands | 47a | 47c completes `init` and adds `rotate-key`, `recover`, `fingerprint`; 46 adds `nft-selftest`, `nft-selftest-sender`, `containment-drill`, `nft-recreate` |
| `domovoi agent selftest`, `--installer`, `--record`, ids ST-01 to ST-20 | subcommand, ids | 47a | 47b (adds listing row `SK-01`, never gating), 46 |
| `domovoi skill-runner serve`, hidden `domovoi agent dirs` | subcommands | 47b | none |
| `RunnerRequest`, `RUNNER_SOCKET_PATH`, the runner `RefusalCode`s (`run_as_unsupported`, `runner_groups_extra`, ...) | types, tokens | 47b | none (runner statuses reach the hub only through the existing `skill_runner_status`) |
| `domovoi-helper.socket`, `domovoi-helper@.service` | units | 47a (baseline text) | 47d amends it with a marked candidate delta (decision OD-1) |
| `domovoi-skill-runner.socket`, `domovoi-skill-runner@.service` | units | 47b | none |
| `domovoi-helper-selftest.service` | unit | 46 | none |
| `HELPER-1`, `HELPER-2`, `IDENT-1`, `ID-2`, `ID-3`, `UNIT-2` (docker clause), `UNIT-3` (helper), `UNIT-4`, `UNIT-5` (helper socket), `UNIT-7`, `UNIT-8`, `UNIT-9`, `INSTALL-1`, `SELFTEST-1` | arch rules | 47a | 47b, 47c, 47d, 45, 46 run over them |
| `UNIT-1`, `UNIT-6`, `RUNNER-1`, `RUNNER-2`, runner clauses of `UNIT-2`, `UNIT-3`, `UNIT-5` | arch rules | 47b | none |
| `ATTEST-1` to `ATTEST-10` | arch rules | 47c | 46 (`ATTEST-6` scan lands with 46) |
| `READ-1`; updated `UNIT-3` table and `HELPER-1` path | arch rules | 47d | 45, 46 |
| `UNIT-3` clause for the selftest unit | arch rule clause | 46 | none |
| `domovoi_core::attest::{SafetyIdentity, ExpectedSafety, Hash32, BootId, UnverifiedReason, Nonce32, decide, unprotected_paths}` | module | 47c | 46 (reads them), 45 (passes `expected_safety = None` only) |
| `HelperTrust` service (`view`, `recheck`, `pin`, `unpin`, `fresh`), `AttestedCoverage`, `GuardRefusal { MalformedSafety, SafetyChanged, SafetyUnknown }`, `check_expected` | hub, helper | 47c | 46 |
| `RefusalCode::PolicyRequiresVerifiedSafety` (existing wire code) | code | existing | 47c, 46 (`unverified_requires_human` is a rejected alias) |
| `POLICY_DISPATCHABLE`, `is_policy_dispatchable` | const, fn | 45 (`["unit.restart"]`) | 46 appends `firewall.block_source` |
| `PermitIssuer`, `PolicyPermit::restore`, `UnitRelay`, `execute_unit_restart`, `unit.identity`, `unit.restart` | ports, fn, contracts | 45 | 46 follows the pattern with siblings (there is no generic dispatch) |
| tokens `platform_untested`, `platform_blocked` | agent-side tokens | 45, mapped from 47a's `Unavailable(WriteGroupNeedsOptIn / PlatformBlocked)` | 46 |
| `firewall.block_source`, `firewall.list_blocks`, `ContainCall`, `ContainRelay`, `ContainmentPlan`, `containment_blocks` | contracts, types, table | 46 | none |
| `DockerReadApi`, `PrivilegedReads`, `ReadToken`, `ContainerRef`, `DeviceRef`, `HelperContainerSource` | ports, types | 47d | none |
| CI jobs `privileged`, `installer`, `units` | CI | 47a (`privileged`); `installer`, `units` exist | 47b and 47d add rows to `privileged`; 47c's `privileged_custody.rs` runs in it |
| CI jobs `containment-ns`, `containment-leak` | CI | 46 | none |
| migrations `helper_trust` (expected 0020), `permits_and_autonomy` (expected 0021), `containment_blocks` (expected 0022) | files | 47c, 45, 46 | none |
| ADRs 0033 to 0038 | files | 47a, 47b, 47c, 47d, 45, 46 in that order | none |

## Test ownership

Every named test has exactly one owning brief. A test extended by a later brief stays with its first owner and the later
brief says "extended here".

- **RW-37 list (76 names, `../03-architecture-decisions.md`):** 47c owns 60 (its section 12.2, which maps every name),
  46 owns 15 (46 section 12.1: the two overlap names, the four unverified-path names, the unattended, agent-label,
  exclusion-overlap and `no_management_cidrs` names, the uncovered-paths name and the four limit names), 45 owns 1
  (`mutation_limit_survives_helper_restart`, 45 section 12.1). 60 + 15 + 1 = 76. Before this pass 46's brief named one of
  its 15 by the exact RW-37 spelling and 45's brief named none of its one; the three counts now agree.
- **Duplicates removed:** `startup_check_*` (four names), `unit_parser_flags_runtime_max_with_oneshot`,
  `unit_parser_flags_wide_socket_mode`, `spawned_skill_gets_no_listen_vars_and_no_connection` and the adoption tests
  belong to 47a; 47b's own counterparts are `runner_start_check_*`, `unit_parser_flags_wide_runner_socket_mode` and
  `serve_spawned_skill_gets_no_listen_vars_and_no_connection`. `shipped_units_pass_every_unit_rule` and
  `rules_file_change_has_an_adr` are 47a's gates; the others keep them green. `helper_request_has_no_write_or_free_form_variant`
  and `helper_config_accepts_the_documented_example` are 47a's tests, extended by 47d.
  `mutation_limit_counts_apply_only` (46) no longer claims the restart-persistence half, which is 45's test.
- **Synthesis test plans (sections 7 of syntheses 01 to 04):** all named tests are owned by a brief, by exact name or by
  the renamed equivalents below. Names not found verbatim:

| Synthesis name | Owner and the name used there |
|---|---|
| 01: `enabled_runtime_unit_is_refused_unless_the_owner_accepts_it` | 45 `enabled_runtime_unit_is_refused` (the owner decision R-8 Q2 refuses it outright, no acceptance path) |
| 02: `agent_cannot_read_helper_key_list_directory_or_proc` | 47a `agent_cannot_read_helper_state_directory_or_proc` |
| 02: `runner_unit_binds_no_socket`, `runner_unit_declares_no_supplementary_groups`, `runner_unit_has_no_inaccessible_path_under_masked_parent`, `runner_unit_requires_protect_home_yes`, `runner_unit_type_is_simple_when_runtime_max_sec_is_set` | 47b `unit_parser_flags_socket_bind_into_runner` (UNIT-6), `unit_parser_flags_nonempty_supplementary_groups`, `unit_parser_flags_rw39_inaccessible_child_under_masked_parent` (UNIT-1), `unit_parser_flags_missing_protect_home` (UNIT-3), 47a `unit_parser_flags_runtime_max_with_oneshot` (UNIT-4) |
| 02: `runtime_max_sec_stops_simple_unit_and_kills_grandchild` | 47b `corrected_units_start_isolate_cancel_time_out_and_clean_up` |
| 02: `skill_with_agent_group_and_no_mask_is_stopped_by_helper_uid_check` | 47a `skill_given_agent_group_is_stopped_by_helper_uid_check` |
| 02: `untested_platform_allows_skills_with_recorded_acknowledgement_and_refuses_write_groups_without_opt_in` | 47a `untested_platform_allows_skills_with_notice_and_refuses_write_groups_without_opt_in` (the acknowledgement itself is enforced and tested by 47b) |
| 02: `docker_group_probe_reads_helper_key_through_bind` | **deferred**: a canary that documents root-equivalence of the `docker` group and needs a Docker host; no brief builds it; suggested owner: a later pass of the `privileged` suite (see Known gaps) |
| 03: `same_key_repin_clears_nothing_needed` | **not a test**: a row label of 03 X-1 meaning "nothing to clear"; covered by 47c `mismatch_cleared_by_next_verified_attestation` and `agent_reenrollment_keeps_helper_pin` |
| 04: `leak_self_test_result_is_keyed_by_kernel_nft_and_layout` | 46 `selftest_result_is_keyed_by_kernel_nft_and_layout` |

## Cross-brief decisions

Where two briefs disagreed and the decision register or a synthesis did not settle it, the narrower or safer reading was
taken. The owner may overturn any of these; each brief already states its own part.

1. **Helper spawning.** One statement, in 47a section 5: the helper spawns nothing in 47a; a later brief may add only the
   fixed programs it names, through the injected `ProcessExecutor`, never a shell, never from a request-supplied path (47c
   `ip`, 47d `smartctl`/`dmesg`/`ss`, 45 `systemctl`/`busctl`, 46 `nft`/`conntrack`). `R3` and `R4` do not change.
2. **Helper unit file.** 47a owns it and ships the baseline (`CapabilityBoundingSet=CAP_NET_ADMIN`, `PrivateDevices=yes`).
   47d amends it with a marked candidate delta (more capabilities, `PrivateDevices=no`, `DevicePolicy=closed`) that stays
   unmeasured until the privileged rows run; the owner confirms OD-1 at 47d's launch gate. 45 and 46 need no unit change
   (46's namespace work lives in its own one-shot unit).
3. **Frame cap.** The helper's 65,536 bytes is the *request* cap; 47d adds a 4 MiB response cap. 47b's runner protocol
   has its own constants in its own module.
4. **`unit.restart` post-restart check.** 45 follows REQ-20 (read `/proc/<ExecMainPID>/environ` and compare with the bound
   values; never re-read the loaded properties). `04` §8.3 and synthesis 00 §6 still say "loaded properties" and are wrong
   on this point; correcting them is an owner follow-up (45 section 15).
5. **Persisted limiter.** 45's first draft kept the restart ring in memory, which cannot work because the helper is one
   process per connection. 45 now persists it in the `restarts` file under `state.lock`, owns the RW-37 test, and
   interprets `[limits] mutations_per_hour`: **`0` means no write operation is accepted** (fail closed, like
   `max_active = 0`). 46 counts `apply` against the same file. The owner may prefer "0 = unlimited"; that needs a different
   default in 47a's `helper.toml`.
6. **Tier gate tokens.** The helper answers through 47a's `Unavailable { group, reason }` (`WriteGroupNeedsOptIn`,
   `PlatformBlocked`); the agent maps them to 45's `platform_untested` and `platform_blocked`. `UnitRefused` never carries
   them.
7. **Refusal spelling for unverified policy runs.** The existing wire code `PolicyRequiresVerifiedSafety`; the RW-37 name
   `unverified_requires_human` is not added (47c and 46 agree). `malformed_safety`, `safety_changed` and `safety_unknown`
   are 46's helper-side refusals built on 47c's `GuardRefusal`.
8. **Generic dispatch.** 46 assumed 45 builds a generic helper-backed dispatch; 45 builds a unit-specific one
   (`UnitRelay`, `execute_unit_restart`, `requests/unit.rs`). 46 now builds sibling code (`ContainRelay`,
   `execute_containment`, `requests/containment.rs`) and appends `firewall.block_source` to 45's `POLICY_DISPATCHABLE`.
9. **`max_active`.** Default `0` everywhere (47a config, 47c report default, 46 contract); the `50` in `04` §8.3 is an
   example agent ceiling, not a default.
10. **Launch gates.** "Accepted for building" is defined once, in 47a's "Launch gate", and 47b and 47d cite it. R-10 gates
    47c only; R-8 gates 45; R-2 and R-10 gate 46.
11. **Platform tiers.** No platform is `Supported` while `SUPPORTED_PLATFORMS` is empty (47a); 45 and 46 rely on the
    per-host opt-in `[platform] allow_write_groups_on_untested`.
12. **`--base` for `arch-check`.** Every brief uses `--base origin/overhaul-investigation` (the pull request target,
    index §1); `origin/main` was a slip in 47a and 47d.
13. **Selftest unit path.** 46's `domovoi-helper-selftest.service` uses `/usr/local/bin/domovoi`, rewritten by the installer
    like 47a's units, so `UNIT-9` and `ID-3` apply to it.
14. **Post-47d reads.** 45 replaces 47d's test `systemd_read_stays_not_implemented` and extends `HelperRequest::group()`.

## Known gaps and deferred work

Work that the decision register or the syntheses mention and no brief owns. None blocks a brief; each needs an owner
before the area ships.

| Gap | Where it comes from | Why it is not owned | Suggested owner |
|---|---|---|---|
| `04` §8.3 table rows (the `docker.read`, `smart.read`, `kernel.read`, `net.read`, `systemd.*` and `nft` group rows, the post-restart wording) and the `04` §8.4 edits | 45, 46, 47b and 47d list them as follow-ups, none makes them (planning texts are outside the briefs) | planning documents, edited once for all | the owner, in one pass after 46 merges; each brief's section 15 lists its part |
| Journal vacuum and container restart skills lose root access when the agent stops being root | 47b notes the starter skills `restart_container`, `restart_unit`, `vacuum_journal` and the journal readers; documented, not fixed | no helper operation exists for them (D-24 keeps `container.restart` unavailable unattended; `unit.restart` is 45's only write) | a follow-up brief after 45: helper operations or per-skill documentation; owner decision |
| `security.fail2ban` check needs a root-only socket (finding F-1 of 47d) | 47d: no helper group covers it; the check reports an error on a migrated host | no `fail2ban.read` group in RW-39 | a later helper-read brief plus an edit of `04` §8.3 |
| Audit row for the per-host write opt-in when the owner changes it | 47a ships no hub change; 47d says it belongs to 45; 45 records the opt-in only in the dispatch audit (`platform_optin_is_in_the_dispatch_audit`) | no brief records the change itself | 45 follow-up, or 47c's Helper tab in the Capability Center (hub reads `write_opt_in` from the `Groups` answer) |
| Hub-visible `run_as_unsupported` | 47b keeps the wire unchanged: no `run_as` in `hello.skills`, no new failure code; only the selftest row `SK-01` and the agent-side message | would need a wire change in 1.2 (still unreleased) | the owner decides; then a small brief after 47b |
| Package skills through the runner | 47b implements `SkillSource::Package` in the runner and tests it there; nothing sends it, and `skill_runner_not_integrated` stays | needs 41/42 changes beyond 47b's scope | a brief after 47b that switches the packages to the runner and removes the reason |
| `ReadGroup` after 47d | 47d removes `Read { group }`; `ReadGroup` may become dead code | 47d does not say | 47d (delete it if nothing references it, report it) |
| Real-installer evidence for the helper, runner and `unit.restart` on Debian 12, Ubuntu 22.04, systemd 258, a non-merged `/usr` host and an SELinux host | synthesis 00 §8; every brief lists it as an exit criterion or an open acceptance condition | needs hosts | the owner, per the validation matrix |
| `docker_group_probe_reads_helper_key_through_bind` canary | synthesis 02 section 7.1 | needs a Docker host; documents root-equivalence only | the `privileged` suite, after 47a |
| `link_peer` source for the attestation (47c E5) | 47c: none in this release | owner default pending | 47c's launch gate |
| Docker stats, name-to-id, images; rootless or user Podman sockets; widening the env sanitiser (F-2) | 47d non-goals | deliberately out | later helper-read work |

## Standard blocks every prompt contains

Each prompt copies these sections verbatim, then adds task-specific items.

- "Execution constraints"
- "Repository rules"
- "Completion report"

The text of each block is in `_blocks.md`.
