# 47a — Helper accounts, directories and root-helper skeleton

| | |
|---|---|
| Branch | `helper-accounts-and-skeleton` |
| Milestone | H1 (first of four: 47a accounts, sockets and helper skeleton; 47b skill runner; 47c safety attestation; 47d read operations; see `../07-implementation-prompt-index.md` §6.4 and `../investigations/synthesis/00-overview-and-implementation-plan.md` §6) |
| Depends on (must be merged) | 44 (`helper-adrs`: ADRs 0030, 0031 and 0032, updated from the research; they are still `proposed`). 20 (wire 1.2: `HelloExtensions::{helper_status, skill_runner_status, runs_as_root, helper_identity}`, `agent_status`; verified present in `crates/domovoi-proto/src/messages/handshake.rs` and `crates/domovoi-agent/src/link/handshake.rs`). 22 (listed by the index; 47a consumes none of its types). 41 and 42 are merged on the base this brief was checked against (`crates/domovoi-agent/src/packages/` exists); 47a changes how their `PrivilegeBoundary` is computed (section 3, difference 8). |
| Can run in parallel with | nothing in wave L. Series order: 47a, 47b, 47c, 47d, 45, 46 (`README.md`, "Launch order"). 47b, 47c and 47d start only after 47a is merged; 45 starts after 47c and 47d; 46 starts after 45. |
| Produces (shared contracts) | `domovoi_core::identity::*` (account and group names, the path table of synthesis 02 §5.7, refusal tokens); `domovoi_core::helper::*` (helper protocol: `HelperRequest`, `HelperResponse`, `HelperGroup`, framing, `RefusalToken`); `domovoi_core::isolation::*` (check ids ST-01 to ST-20, `CheckResult`, `SelftestReport`, `IsolationReport` and its evaluator, `BoundaryState`, `PlatformTier`, `classify_platform`); port `domovoi_core::ports::HostInspect`; new crate `domovoi-helper` (`adopt`, `serve`, `state`, `start_check`, `repair`, `selftest`, `probe`, the `HostInspect` adapter); `domovoi_config::HelperConfig` (`/etc/domovoi/helper.toml`); `domovoi_agent::helper_client` and `domovoi_agent::state_owner`; subcommands `domovoi helper serve|init|repair` and `domovoi agent selftest`; units `deploy/systemd/domovoi-helper.socket` and `domovoi-helper@.service`; the installer's three-identities layout and `--migrate-identity`; arch rules `HELPER-1`, `HELPER-2`, `IDENT-1`, `ID-2`, `ID-3`, `UNIT-2` (docker clause), `UNIT-3` (helper unit), `UNIT-4`, `UNIT-5`, `UNIT-7`, `UNIT-8`, `UNIT-9`, `INSTALL-1`, `SELFTEST-1` and their ADR; the privileged test script `deploy/test/identities.sh` |
| Consumes | ADRs 0030 and 0031 as the contract; synthesis 02 §5.3, §5.7, §5.8, §5.9, §5.9.1, §5.10, §5.14 and §7; synthesis 05; decisions D-24, D-29, D-31, D-36 to D-43 and the owner's decisions of synthesis 00 §5a and §5b; `domovoi_core::ports::{ProcessExecutor, ProcessSpec}` (the only spawn path); the agent's existing `HelloExtensions` and `AgentStatusSource` |
| Launch gate | R-3 and R-9 accepted by the owner **for building** (index §3 wave L; "accepted for building" is defined in section 2, "Launch gate", and is the one meaning every later brief of the series uses). 44 merged. See "Launch gate" in section 2 for what "accepted" still needs and how its circular items are closed by this prompt. R-10 is **not** a gate for 47a (it gates 47c). |
| Suggested model | large (security boundary, installer and migration; every decision is made below, follow it exactly). The mechanical parts (the unit parser rules, the token table tests, the docs) may go to a mid-size subagent with a well-specified file list. |

## Hub use-case layer (read first)

This prompt changes no hub code. Prompt 29 (`use-case-layer`) organised `crates/domovoi-hub/src/services/`; its rules (`MOD-1` to `MOD-4`, `CALLER-1`) stay in force and are not touched. If you find that a change here needs an edit under `crates/domovoi-hub/`, stop and report: it means the design has leaked a helper concept into the hub, which `HELPER-1` forbids. The helper, the accounts and the isolation checks are host-local; the hub learns about them only through the existing `agent_status` fields.

## 1. Objective

Put the operating-system boundary of RW-39 in place and make it checkable, without yet moving any skill or attestation onto it. Concretely, on one host:

1. **Three identities exist and are verified.** The installer creates the unprivileged `domovoi-agent` account and the group-less `domovoi-skill` account, creates every directory with the modes of synthesis 02 §5.7, and never repairs what it finds (it reports found and expected mode and owner). A new `domovoi agent selftest` re-asserts all of it (ST-01 to ST-20).
2. **A root helper skeleton runs behind a socket-activated, sandboxed unit.** `domovoi helper serve` adopts the connection from fd 0 with std only, checks `SO_PEERCRED`, reads one length-prefixed JSON request, answers it from a **closed** request enum and exits 0. Only `health`, `groups`, the read-group dispatch (answered `unavailable`) and a `safety.attest` stub (answered `unavailable`) exist. There is no write group.
3. **The helper fails closed at start** on a wrong directory, key or state mode (token `helper_state_permissions`), reports each offending path with found and expected mode and owner, and never repairs at start. A **separate, owner-confirmed local command** (`domovoi helper repair`) does the repair.
4. **Platforms are classified at run time**, not by a version list: a library function turns a self-test report into Supported, Untested or Blocked, with a per-host switch (off by default, recorded) for helper write groups on Untested platforms.
5. **An existing single-account install migrates safely** and is labelled "privilege boundary not in effect" until the owner does it.
6. **The rules are enforced**: new `arch-check` rules and gate tests, with the ADR that `xtask/src/arch/rules.rs` changes require.

It does **not** build the skill runner, the attestation, any helper write group or any containment (section 5).

## 2. Architectural context

**The three identities (ADR 0030, RW-39).**

| Component | OS user | Reaches | Must not reach |
|---|---|---|---|
| control agent (`domovoi agent run`) | `domovoi-agent`, supplementary groups exactly `systemd-journal` (when that group exists), never `docker` | hub link; helper socket; its `state_dir` (0700); `packages_dir` (writes) | helper state and key; `/etc/domovoi/helper.toml` |
| skill runner (`domovoi skill-runner`, **47b**) | `domovoi-skill`, no supplementary group of any kind, never in `docker` | skill and package files read-only | agent state, helper state, both sockets, `/etc/domovoi` except `skills/` |
| helper (`domovoi helper serve`) | root | `/var/lib/domovoi-helper` (0700), `/etc/domovoi/helper.toml` | executing arbitrary commands, `run_as`, `sudo`, `systemd-run` |

Three layers decide who may connect to a socket, and none replaces another: socket mode and group (`root:domovoi-agent 0660`, a hard maximum, F-ACT-4, F-MSK-13); `SO_PEERCRED` (the helper compares the peer uid; it separates users, not processes); the mount-namespace mask in the runner unit (47b). The agent is kept from the helper state and key **by file permissions alone** (a mask in the agent unit would remove the agent's own sockets, so `04` §8.3's earlier wording is withdrawn). The helper allowlist bounds operations; it is not execution authorization (D-29).

**Decisions that are final (synthesis 00 §5a, `99` D-36 to D-43; do not reopen).**

| Decision | Consequence in 47a |
|---|---|
| `Accept=yes`, helper follows the runner (D-36) | one helper process per connection; no in-memory state outlives a request; the lock is held across a request |
| `/etc/domovoi` stays `root:domovoi-agent 0750`, the mask stays mandatory (D-37) | the agent reaches `/etc/domovoi` through group `domovoi-agent`; `agent.toml` must be readable by that group (difference 4) |
| wrong directory, key or state mode: refuse, report, owner-confirmed repair, never at start (D-38) | `start_check`, `helper repair`, ST-10, ST-11 |
| platforms are tiers gated by a runtime check (D-39); per-host opt-in for write groups, off by default, recorded in audit | `classify_platform`, `[platform] allow_write_groups_on_untested` |
| inherited socket: std only from fd 0, refuse a non-zero `LISTEN_FDS`, no new dependency, no `unsafe` (D-40) | `domovoi_helper::adopt` |
| no user namespaces, no AppArmor or SELinux profile (D-41) | nothing to ship; ST-19 only records |
| per-package narrowing deferred; ST-20 warns above a median of about 100 ms, never blocks (D-42) | ST-20 |
| five-site spelling edit to `domovoi-skill` (D-43) | belongs to **47b** (index); 47a defines the one constant and the rule `IDENT-1` so that the edit is checkable |
| token for a wrong state mode is `helper_state_permissions` (synthesis 00 §5a, ADR 0032 E2) | one token across helper start, ST-10/ST-11 and the hub |

**Where the code lives (ADR 0030 open question A2, decided by this brief; the owner confirms it at the launch gate, and step 1 records it in an ADR).** One new crate, `domovoi-helper`, holds the root helper runtime, the real `HostInspect` adapter, the self-test and the repair command. Pure types, the tier function and the check evaluators live in `domovoi-core` (pure, no I/O). The agent gets a small client module. The binary `domovoi` is the composition root and injects the `ProcessExecutor` (R3, R6). Reasons: a root process must not link the agent's network stack; adapters may not depend on each other (R1), so the agent cannot reuse a helper adapter and the helper cannot reuse the agent's `fs_atomic`; the runner (47b) needs the same inspection port, which the binary injects. `ARCHITECTURE.md` §4 gains one row (`domovoi-helper`: `core`, `config`) and `domovoi` gains the edge to it.

**Boundary state versus tier.** Two different things, reported separately. `BoundaryState` says whether the agent runs as `domovoi-agent` with exactly the expected groups (ST-01, ST-02); when it does not, the host is labelled **privilege boundary not in effect** (RW-39, `04` §7.3) and gets no unattended execution. `PlatformTier` says how far the host's isolation checks go. A legacy root agent has boundary `NotInEffect` and is not "Blocked": it keeps working as today.

**Platform tiers as code (`0031` Decision 7, synthesis 00 §5b).**

| Tier | Qualifies | Skills | Helper read groups | Helper write groups |
|---|---|---|---|---|
| Supported | on `SUPPORTED_PLATFORMS` and the gate passes | allowed | allowed | allowed once their own gates pass (45, 46) |
| Untested | systemd host, not listed, every required check `pass` | allowed with a notice and a recorded acknowledgement | allowed | refused unless `[platform] allow_write_groups_on_untested = true` |
| Blocked | any required check `fail`, a required check `nd`, no systemd, or a container without a measured design | refused | allowed only if ST-10 to ST-12, ST-15 and the helper start checks pass | refused |

Required checks: ST-01 to ST-07, ST-09 to ST-16, ST-18, plus the helper start checks. `nd` never satisfies a required check. `SUPPORTED_PLATFORMS` is **empty** (nothing is release-tested; a platform is promoted only after a real-installer pass). Until 47b ships, ST-13 and the runner halves of ST-07 and ST-14 are `nd`, so every real host is Blocked **for skills** by construction, which is correct because no runner exists; the helper read decision is independent of them.

**Launch gate: what "accepted" still needs, and how this prompt closes the circular part.** R-3 and R-9 are open in `99`. ADR 0031 "What acceptance still needs" lists five items; ADR 0030 lists five. They split in two:

| Item | Needs the output of | Closed by |
|---|---|---|
| a real-installer pass with the production helper, runner and installer on the measured hosts, creating the real accounts and running `selftest --installer`; a platform is promoted to Supported only after it | 47a and 47b | exit criterion: 47a runs `deploy/test/identities.sh` on at least one real host (a root-capable VM with systemd as PID 1) before merge and records the result in the PR; the runner rows wait for 47b; **no platform becomes Supported** |
| the checks ST-01 to ST-20 implemented with the three corrected wordings, and run in the pass | 47a (and 47b for the runner halves) | acceptance criteria 6 to 8 below |
| the production `getgroups` start check and the fd 0 adoption running in a real unit | 47a (helper side) and 47b (runner side) | `identities.sh` rows `helper_adopts_fd0_in_a_real_unit` and `helper_start_check_runs_in_a_real_unit` |
| the key-custody test with the real helper binary | 47a (the permission half) and 47c (the key) | `agent_cannot_read_helper_state_directory_or_proc` runs against the real binary and a fixture key in 47a; the production key test is 47c's |
| A2 decided in its own ADR | the owner | step 1 of this prompt writes it; the owner accepts it at the gate |
| B5: the helper directive set per operation group measured | the first prompt that adds a helper write group (45, 46) | 47a ships the **measured baseline** unchanged (synthesis 02 §5.3) and pins it with `UNIT-3`; narrowing is not attempted |
| unproven items (systemd 252 and 249, SELinux, other filesystems, power loss, `MaxConnections` behaviour, the reload-pending ST-07 case) | more hosts | recorded as residual in the ADR revision; the owner accepts them explicitly at the gate |
| the published platform list and the notice wording | the owner | open decision B2 of ADR 0031: this brief ships the list as an empty constant and the wording in section 6.9, flagged **convention**; the owner may amend before merge |

So the owner's acceptance of R-3 and R-9 at the gate means: the decisions of ADRs 0030 and 0031 are final for building, the unproven items are recorded as residual, and the production-binary evidence is an exit criterion of 47a and 47b, not a precondition.

## 3. Repository context

Verified in the checkout this brief was written against (branch `research-synthesis`, HEAD `b8021381`, workspace version `0.0.10`, `Cargo.toml:6`). Re-open each item before editing and report differences.

**What exists today.**

- `install.sh:91-128` (`agent_unit`) prints a unit with **no `User=`**; `install.sh:120-121` set `ReadWritePaths=/var/lib/domovoi-agent` and `ProtectSystem=full`. The header comment (`deploy/systemd/domovoi-agent.service:5-6`) says "It runs as root". `deploy/test/installer.sh:48-52` asserts the embedded units equal `deploy/systemd/`.
- `install.sh:324-328`: the agent role creates `/etc/domovoi` `0750 root:root`, `/etc/domovoi/skills` `0750 root:root`, `/var/lib/domovoi-agent` `0700 root:root`, and copies the starter skills. It creates **no** `/var/lib/domovoi-packages`, no `domovoi-agent` or `domovoi-skill` account, no helper unit and no `helper.toml`. `make_dir` (`install.sh:285-291`) creates only when absent. `install.sh:4-6` says it "never writes a config file".
- `install.sh:308-323`: the hub role creates user `domovoi` and `/etc/domovoi` `0750 root:domovoi`. Both roles share `/etc/domovoi` (difference 7).
- `deploy/systemd/` holds `domovoi-hub.service`, `domovoi-agent.service` and `single-box.conf` only. No helper or runner unit exists.
- `crates/domovoi-core/src/skill/manifest.rs:102` (doc comment) and `:149` (`run_as: Option<String>`); `crates/domovoi-core/src/ports/process.rs:26` (`ProcessSpec.run_as`) and `:61` (`ProcessError::UserSwitch`). `crates/domovoi-runner/src/exec/spawn.rs:19-35` (`resolve_user`) refuses a switch unless the process is root ("agent must run as root to switch users", `:29`); `exec/mod.rs:94-95` calls it. The spawn code uses `.env_clear()`, `.process_group(0)` and a null or piped stdin, never inherit (`spawn.rs:51-58`); that is what keeps `LISTEN_*` and the connection from a child (synthesis 05 §1).
- `domovoi-skills` (with the `s`) still appears in five places: `manifest.rs:102`, `crates/domovoi-core/tests/manifests.rs:16`, `crates/domovoi-runner/tests/runner/skill_run.rs:16` and `:74`, and `crates/domovoi-core/tests/snapshots/manifests__skill_manifest_json_snapshot.snap:53` (matches ADR 0030 Decision 3). They are edited by 47b.
- `crates/domovoi-agent/src/link/handshake.rs:25-58`: `HelloExtensions` already carries `skill_runner_status`, `helper_status`, `runs_as_root`, `helper_identity`; the default is `Absent`, `Absent`, `false`, `None`. `crates/domovoi-proto/src/messages/handshake.rs:224-270` defines `SkillRunnerStatus`, `HelperStatus { Absent, Ok{ops}, Error{reason} }` and `HelperIdentity`. `crates/domovoi-agent/src/link/mod.rs:57-61` documents `STATUS_PROBE_INTERVAL` (60 s) and says a later milestone supplies mutable component status "through the same source" (`AgentStatusSource`, `handshake.rs:80-117`). `crates/domovoi-agent/src/runtime.rs:106-113` computes `runs_as_root()` with `nix::unistd::geteuid()`.
- `crates/domovoi-agent/src/packages/mod.rs:262-269`: `privilege_boundary()` is `InEffect` whenever `!runs_as_root`. `PrivilegeBoundary` is `crates/domovoi-core/src/package/output.rs:77`. `crates/domovoi-agent/src/packages/layout.rs:140-175` requires `packages_dir` to be owned by the agent's uid and not a symlink.
- `crates/domovoi-agent/src/config_file.rs:77` and `:127`: `agent.toml` is written `0o640` on Unix. `crates/domovoi-agent/src/store.rs:19` writes the identity file `0o600`. `crates/domovoi-agent/src/fs_atomic.rs:12` (`write_atomic`) and `:86` (`write_atomic_durable`, which adds the directory `fsync`) are the agent's atomic writes, with a `DurableFaults` fault-injection struct (`:55-60`); the helper crate re-implements the durable form because it cannot depend on the agent (R1).
- `crates/domovoi/src/cmd/mod.rs` has the subcommands `hub`, `agent` (`run|enroll|status|journal`), `check`, `notify`, `selftest`, `digest`, `audit`, `package`, `openapi`, `schema`, `restore`, `upgrade`, `version`. `crates/domovoi/src/cmd/selftest.rs` is the **hub pipeline** self-test (`domovoi selftest [--offline]`); there is no `agent selftest` and no `helper` subcommand. `crates/domovoi/src/cmd/agent.rs:73`, `:165` (`Run`, `Enroll`) run the agent and `enroll` with the production `SandboxExecutor`.
- `Cargo.toml:77`: `nix = { version = "0.31.3", features = ["signal", "process", "user", "fs"] }`. `nix` is used by `domovoi-agent` (linux), `domovoi-runner` and `domovoi-checks` (`host` feature). The workspace lint is `unsafe_code = "forbid"` (`Cargo.toml:113`). `libc` is not a workspace dependency. `domovoi-core`'s external allowlist (`xtask/src/arch/rules.rs:21-34`) is `serde, serde_json, thiserror, jiff, uuid, async-trait, sha2, hex, secrecy, regex`.
- `xtask/src/arch/rules.rs`: `CRATES` (every workspace member must appear, `:17`), `PROCESS_ALLOWED_PREFIXES` (`:167`), `RAW_OPS`/`BOUNDARY_CARDS` (`:203`, `:287`), `ZONES`/`UNZONED_MODULES` (hub, `:484`, `:515`), `PURE_CRATE_DIRS` (`:567`), `RULES_FILE` and `ADR_DIR` (`:594`, `:597`). `xtask/src/arch/mod.rs:1-60` runs `graph`, `manifest`, `scan` and `git` checks. Nothing parses `deploy/systemd/` or `install.sh` today.
- CI (`.github/workflows/ci.yml:576-604`): job `installer` runs `deploy/test/installer.sh` as root in `ubuntu:24.04` and `debian:12` **containers** (no systemd as PID 1); job `units` runs `systemd-analyze verify` on the two existing units. There is no privileged job.
- `crates/domovoi-testkit/src/process.rs` has `FakeProcessExecutor` (expectations by program and argv, every call recorded); `fs.rs` has `FixtureHostFs`. No fake exists for file metadata, account lookup or `getgroups`.
- `crates/domovoi-agent/src/journal/mod.rs:116` and `crates/domovoi-agent/src/store.rs` create files under `state_dir` as whoever runs the command; `domovoi agent enroll` and `journal init|reset` are documented to run with `sudo` (`docs/book/src/install/binary.md:46`).

**Differences between the planning documents and the repository (state each in the completion report, section 7 step 1).**

1. The planning text puts `ExecStart=` "under `/usr/bin`". `install.sh` installs to `<prefix>/bin/domovoi` (default `/usr/local/bin`) and rewrites the unit with `sed` (`install.sh:336`). The new units keep that mechanism; the rule that matters is "nothing under `/run`, `/tmp` or `/dev/shm`" (`UNIT-9`, ST-09), not the literal `/usr/bin`.
2. `domovoi selftest` already exists and means something else (the hub pipeline). `domovoi agent selftest` is a new, separate subcommand; the top-level one is untouched.
3. The installer's header says it never writes a config file. 47a writes exactly one, `/etc/domovoi/helper.toml`, only when absent (RW-39 migration step 3). The header comment and the book change with it.
4. `agent.toml` is written `0640` today; synthesis 02 §5.7 and `04` §8.3 require `0644 root:root` so that the unprivileged agent can read it through the 0750 parent. 47a changes the two write sites to `0644` and the migration normalises an existing file.
5. The agent role today creates `/etc/domovoi/skills` as `0750 root:root`; the table requires `0755` (the runner must traverse it, F-CUS-8). The migration normalises it.
6. `/var/lib/domovoi-packages` is not created by the installer and an unprivileged agent cannot create it under `/var/lib`. The installer must (synthesis 02 §5.8 step 2).
7. **Hub and a separate agent unit on one host.** The hub installer makes `/etc/domovoi` `root:domovoi 0750`; the three-identities layout needs `root:domovoi-agent 0750`. Both cannot hold. Decision below (section 14): such a host stays on the legacy layout, labelled, and the installer says why. The single-process mode (`deploy/systemd/single-box.conf`) runs the agent as the hub user and is likewise "boundary not in effect".
8. `PackageDeps::privilege_boundary()` counts "not root" as "in effect" (`packages/mod.rs:262`). Under RW-39 the boundary is in effect only when the agent is the `domovoi-agent` account with the expected groups (ST-01, ST-02). 47a feeds the real `BoundaryState` into it; the wire field `runs_as_root` stays as is.
9. The enroll and journal owner commands run as root and would leave root-owned files in a `state_dir` that the unprivileged agent must own. 47a adds `state_owner::hand_over` (section 6.7).
10. ADR 0030 records that `helper_identity` is only the optional announce field "set to `None`"; today `HelloExtensions` and `AgentStatusSource::default()` (`handshake.rs:117`) always report `helper_status = Absent`. 47a replaces the helper value with a start-time probe (section 6.8) and leaves `helper_identity` at `None` (the key is 47c's).

## 4. Exact scope

Files to **add** (non-test files at most 500 lines; split rather than exceed).

| File | Purpose |
|---|---|
| `crates/domovoi-core/src/identity/{mod,accounts,paths,tokens}.rs` | account and group names (one module, ID-2), the path table (6.1), refusal and check tokens |
| `crates/domovoi-core/src/helper/{mod,group,request,response,frame}.rs` | helper protocol types and framing (6.2) |
| `crates/domovoi-core/src/isolation/{mod,check,report,probe,boundary,tier,evaluate}.rs` | check ids, results, report, the eight-point isolation evaluator, boundary state, tier (6.4, 6.5) |
| `crates/domovoi-core/src/ports/host_inspect.rs` | the `HostInspect` port (6.3) |
| `crates/domovoi-config/src/helper.rs` | `HelperConfig` (6.6) |
| `crates/domovoi-helper/` (`Cargo.toml`, `src/{lib,adopt,start_check,repair,probe,platform}.rs`, `src/serve/{mod,peer,dispatch}.rs`, `src/state/{mod,lock,atomic,init}.rs`, `src/inspect/{mod,unix}.rs`, `src/selftest/{mod,agent_checks,root_checks,units,duration,render}.rs`) | the crate (6.7 to 6.10) |
| `crates/domovoi-agent/src/helper_client.rs`, `crates/domovoi-agent/src/state_owner.rs` | agent-side client and state-ownership hand-over (6.7, 6.8) |
| `crates/domovoi/src/cmd/helper.rs` | `domovoi helper serve|init|repair` |
| `crates/domovoi-testkit/src/host_inspect.rs` | `FakeHostInspect` (6.3) |
| `deploy/systemd/domovoi-helper.socket`, `deploy/systemd/domovoi-helper@.service` | the units (6.11) |
| `deploy/test/identities.sh` | privileged real-account test (section 12) |
| `xtask/src/arch/{units,install_script,identity_rules}.rs` | the unit parser, the `install.sh` parser and the new rules (6.12) |
| `docs/adr/0033-helper-crate-and-isolation-rules.md` (expected number: 0030 to 0032 are in an open pull request, so 0033 is the next free one; list `docs/adr/` at implementation time and take the next unused number if another has merged) | required (rules change, new crate, A2) |
| tests: `crates/domovoi-helper/tests/*.rs`, `crates/domovoi-core/src/{identity,helper,isolation}/tests.rs`, `crates/domovoi-config/tests/helper.rs`, `crates/domovoi-agent/tests/agent/helper_client.rs`, `crates/domovoi/tests/helper_cli.rs`, `xtask/src/arch/units_tests.rs` | section 12 |

Files to **change** (minimal edits): root `Cargo.toml` (none expected: all dependencies exist; the new crate is picked up by `members = ["crates/*"]` and gets a `[workspace.dependencies]` path entry like its siblings), `crates/domovoi-core/src/{lib.rs,ports/mod.rs}`, `crates/domovoi-config/src/lib.rs`, `crates/domovoi/{Cargo.toml,src/cmd/mod.rs,src/cmd/agent.rs}`, `crates/domovoi-agent/src/{lib.rs,runtime.rs,config_file.rs,enroll.rs?,link/handshake.rs?,packages/mod.rs}` and the `journal init|reset` call sites in `crates/domovoi/src/cmd/agent.rs`, `crates/domovoi-runner/tests/runner/exec.rs` (one test, 6.7), `crates/domovoi-testkit/src/lib.rs`, `xtask/src/arch/{mod,rules}.rs`, `install.sh` (units, accounts, directories, `--migrate-identity`, `--print-unit`), `deploy/systemd/domovoi-agent.service`, `deploy/test/installer.sh`, `.github/workflows/ci.yml` (classifier outputs and the `privileged` job, section 12), `deploy/README.md`, `ARCHITECTURE.md`, docs (section 15).

## 5. Explicit non-goals

- **No skill runner** (47b): no `domovoi skill-runner` subcommand, no `domovoi-skill-runner.socket` or `@.service`, no runner request type, no EOF-cancel or process-group code, no `RuntimeMaxSec` runner unit, no `skill_runner_status` other than `Absent`, no `Unavailable(run_as_unsupported)`, no change to which skills run, **no edit of the five `domovoi-skills` sites** (D-43), no mask or `TemporaryFileSystem=` text. `UNIT-1`, `UNIT-6`, `RUNNER-1`, `RUNNER-2` and the runner clauses of `UNIT-2`, `UNIT-3` and `UNIT-5` are 47b's (the unit parser from this prompt is reused). The runner halves of ST-05, ST-06, ST-07, ST-13 and ST-14 collect nothing here: the **evaluators** are in 47a and are tested with fakes; the production collection runs inside the runner (47b) and reports `nd` until then.
- **No attestation** (47c): no key generation (`helper init` here creates the directory and the lock only), no `state` file content, no checksum, no generation, no `safety.attest` behaviour (the request is answered `unavailable`), no signing, no hub verifier, no high-water mark migration, no proto change, no `04b` text, no `expected_safety`. The 0032 tokens `state_missing`, `state_corrupt`, `state_key_mismatch`, `key_missing`, `uninitialized`, `key_corrupt`, `state_write_failed` are 47c's. 47a ships the **atomic write helper** and the **lock** they will use.
- **No helper write group and no helper operation that does anything**: no `docker.*`, no `systemd.*` (so no `unit.restart`, no `unit.identity`: 45), no `nft` (no containment, no leak self-test, no `After=nftables.service`: 46), no `smart.read`, `kernel.read`, `net.read` or `docker.read` implementation (47d). Write groups named in `helper.toml` load but are refused at run time (`unavailable{not_implemented}`). **The helper spawns nothing in this prompt.** This is the one statement of the rule for the whole series: a later brief may let the helper spawn only the fixed programs it names, only through the injected `ProcessExecutor`, never a shell and never from a request-supplied path (47c `ip`, 47d `smartctl`, `dmesg` and `ss`, 45 `systemctl` and `busctl`, 46 `nft` and `conntrack`); `R3` and `R4` stay unchanged and each of those briefs says so.
- **No Docker helper** (D-24): no narrow Docker client, no ADR for it; `container.restart` stays unavailable unattended.
- **No change to the agent-hub wire** (`crates/domovoi-proto/` is untouched; wire 1.2 and its golden frames do not change). `helper_status` and `runs_as_root` are existing fields.
- **No hub or UI change**: no helper-key enrollment view, no tier display, no audit row for the write opt-in. The opt-in record's hub side is 45's first consumer (section 6.9 says what the helper exposes so that it can be added).
- **No hardening candidates** (`RestrictAddressFamilies=`, `ProtectKernel*`, `RestrictNamespaces=` for the helper; synthesis 02 §5.3 marks them unmeasured), no per-group unit split, no per-group capability drop (B5).
- **No mask, no `Accept=no`, no `listenfd` fallback.** fd 3 is refused.
- **No platform claim**: `SUPPORTED_PLATFORMS` stays empty; no document says a platform is supported.
- **No containers, Unraid or non-systemd support**: those hosts keep today's agent, labelled; the installer does not touch them (section 14).
- Do not weaken any gate test or arch rule; do not touch merged migrations (there is no migration).

## 6. Required interfaces and contracts

Marked **fixed** (must match, tests pin it) or **convention** (follow repository style). Names of types are fixed unless marked; field order and private helpers are the implementer's.

### 6.1 Accounts, groups and the path table (`domovoi_core::identity`, pure; **fixed**)

```rust
// identity/accounts.rs: the ONLY non-test definition of these literals (IDENT-1, ID-2).
pub const AGENT_ACCOUNT: &str = "domovoi-agent";
pub const SKILL_ACCOUNT: &str = "domovoi-skill";
pub const AGENT_GROUP: &str = "domovoi-agent";      // socket group and /etc/domovoi group
pub const JOURNAL_GROUP: &str = "systemd-journal";
pub const DOCKER_GROUP: &str = "docker";

pub struct UserEntry  { pub name: String, pub uid: u32, pub gid: u32, pub shell: String }
pub struct GroupEntry { pub name: String, pub gid: u32, pub members: Vec<String> }

/// ST-02. `groups` is `getgroups()` of the agent process. Passes only for exactly
/// {primary gid, and the journal gid when that group exists}; gid 0 or the docker gid never passes.
pub fn evaluate_agent_groups(groups: &[u32], primary: u32, journal: Option<u32>, docker: Option<u32>) -> Result<(), Vec<u32>>;
/// ST-03. `groups_of` is the NSS-aware group list of the account (`getgrouplist`), `members_of_any`
/// is every group listing the account as a member. Passes only when the uid is neither 0 nor the
/// agent's, the shell ends in `nologin` or `false`, and `groups_of == [primary gid]`.
pub fn evaluate_skill_account(user: &UserEntry, agent_uid: u32, groups_of: &[u32], member_of_any: bool) -> Result<(), SkillAccountFault>;
/// ST-04. Neither account is listed as a member of the docker group.
pub fn evaluate_docker_membership(docker: Option<&GroupEntry>) -> Result<(), Vec<String>>;
/// ST-05 (evaluator only; the runner calls it from 47b). Exactly [primary gid].
pub fn evaluate_runner_groups(groups: &[u32], primary: u32) -> Result<(), Vec<u32>>;
```

```rust
// identity/paths.rs: the table of synthesis 02 section 5.7. One list; the installer, ST-10,
// ST-11, the helper start check and `helper repair` all read it.
pub enum PathKind { Dir, File, Socket }
pub enum Principal { Root, Agent }                 // resolved to uid/gid at run time, never hard-coded
pub struct PathSpec { pub id: PathId, pub path: PathBuf, pub kind: PathKind,
                      pub owner: Principal, pub group: Principal, pub mode: u32, pub required: Presence }
pub enum Presence { Required, IfPresent }          // key, state, platform.json are IfPresent in 47a
pub fn expected_paths(dirs: &AgentDirs) -> Vec<PathSpec>;
pub struct PathFacts { pub kind: Option<PathKind>, pub symlink: bool, pub mode: u32, pub uid: u32, pub gid: u32 }
pub struct PathFault { pub id: PathId, pub path: PathBuf, pub found: Option<PathFacts>, pub expected: PathFacts, pub why: FaultKind }
pub enum FaultKind { Missing, WrongType, Symlink, Mode, Owner, Group, WriteBitInTree }
pub fn evaluate_path(spec: &PathSpec, found: Result<PathFacts, FactsError>, agent: (u32, u32)) -> Option<PathFault>;
```

| `PathId` | Path (default) | Kind | Owner:group | Mode | Presence |
|---|---|---|---|---|---|
| `EtcDomovoi` | `/etc/domovoi` | dir | `root:domovoi-agent` | 0750 | required |
| `SkillsDir` | `agent.skills_dir` (`/etc/domovoi/skills`) | dir | `root:root` | 0755; tree: no group or other write bit, files 0644 or 0755, owned by root | required |
| `HelperToml` | `/etc/domovoi/helper.toml` | file | `root:root` | 0600 | required |
| `AgentToml` | `/etc/domovoi/agent.toml` | file | `root:root` | 0644 | required on an agent host |
| `AgentState` | `agent.state_dir` (`/var/lib/domovoi-agent`) | dir | `domovoi-agent:domovoi-agent` | 0700 | required |
| `PackagesDir` | `agent.packages_dir` (`/var/lib/domovoi-packages`) | dir | `domovoi-agent:domovoi-agent` | 0755; tree: no group or other write bit | required |
| `HelperDir` | `/var/lib/domovoi-helper` | dir | `root:root` | 0700 | required |
| `HelperLock` | `/var/lib/domovoi-helper/state.lock` | file | `root:root` | 0600 | required |
| `HelperKey` | `/var/lib/domovoi-helper/attest.key` | file | `root:root` | 0600 | if present (47c creates it) |
| `HelperState` | `/var/lib/domovoi-helper/state` | file | `root:root` | 0600 | if present (47c) |
| `HelperPlatform` | `/var/lib/domovoi-helper/platform.json` | file | `root:root` | 0600 | if present (6.9) |
| `HelperSocket` | `/run/domovoi-helper.sock` | socket | `root:domovoi-agent` | 0660 | required while the socket unit runs |

A symlink at any listed path is a fault (`Symlink`), whatever it points to. Files are inspected through a descriptor opened with `O_NOFOLLOW` (the port's `open_facts`), directories with `lstat` semantics. `/etc/domovoi/skills` is also what 47b bind-mounts; its mode and the tree rule are therefore checked here already.

**Tokens (`identity/tokens.rs`, closed enums with `as_str`, one place; fixed spellings).** Full table in section 10. `RefusalToken` (helper protocol and start) and `CheckToken` (self-test results) each carry an `ALL: &[Self]` constant that `SELFTEST-1` reads.

### 6.2 Helper protocol (`domovoi_core::helper`, pure; **fixed** semantics, **convention** field order)

```rust
pub const HELPER_PROTOCOL: u16 = 1;                 // this socket's own version; not the agent-hub wire 1.2
pub const MAX_FRAME_BYTES: usize = 65_536;  // the request cap; 47d renames it MAX_REQUEST_BYTES (same value) and adds MAX_RESPONSE_BYTES
pub const HELPER_SOCKET: &str = "/run/domovoi-helper.sock";

pub enum HelperGroup { DockerRead, DockerWrite, SystemdRead, SystemdWrite, Nft, SmartRead,
                       KernelRead, NetRead, SafetyAttest }       // as_str: "docker.read", "docker.write", ...
impl HelperGroup { pub const fn is_write(self) -> bool /* DockerWrite, SystemdWrite, Nft */; pub fn parse(&str) -> Option<Self>; }
pub enum ReadGroup { DockerRead, SystemdRead, SmartRead, KernelRead, NetRead }

#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum HelperRequest {
    Health,
    Groups,
    Read { group: ReadGroup },                 // placeholder: 47a answers `unavailable`; 47d replaces this variant with nine typed read variants
    SafetyAttest { nonce: Nonce },             // placeholder: 47a answers `unavailable`; Nonce = 64 lowercase hex chars, validated on parse
}
pub struct HelperRequestFrame  { pub v: u16, pub id: Uuid, pub request: HelperRequest }      // deny_unknown_fields
pub struct HelperResponseFrame { pub v: u16, pub id: Uuid, pub response: HelperResponse }

#[serde(tag = "result", rename_all = "snake_case")]
pub enum HelperResponse {
    Health  { protocol: u16, build: String, pid: u32 },
    Groups  { enabled: Vec<HelperGroup>, ignored: Vec<HelperGroup>, write_opt_in: bool,
              platform_tier: PlatformTier, platform_record: PlatformRecordState },
    Unavailable { group: HelperGroup, reason: UnavailableReason },   // GroupDisabled | NotImplemented | PlatformBlocked | WriteGroupNeedsOptIn
    Refused { token: RefusalToken },                                  // peer, frame, version, unknown operation
    StartRefused { token: RefusalToken, faults: Vec<PathFault> },     // a start check failed (section 6.7)
}

pub fn encode_frame<T: Serialize>(value: &T) -> Result<Vec<u8>, FrameError>;   // 4-byte big-endian length, then UTF-8 JSON
pub fn frame_len(header: [u8; 4]) -> Result<usize, FrameError>;                 // rejects > MAX_FRAME_BYTES with FrameTooLarge
pub fn decode_request(body: &[u8]) -> Result<HelperRequestFrame, RefusalToken>; // unknown `op` string -> UnknownOperation; anything else malformed -> FrameMalformed; v != 1 -> UnsupportedVersion
```

- **The request enum is closed and carries no free-form string**: no path, no command, no unit or container name, no argument vector. Write groups have **no** request variant in 47a, so they cannot be requested at all; the later prompts that add one extend the enum together with their typed arguments. A gate test (`helper_request_has_no_write_or_free_form_variant`) pins the current variant set and fails on any `String`, `PathBuf` or `Vec<String>` field in a request type.
- One request per connection (`Accept=yes`): the helper reads exactly one frame, writes exactly one frame, closes and exits 0.
- JSON uses `serde_json`; unknown fields are rejected (`deny_unknown_fields`); the frame length is checked **before** any body byte is read.

### 6.3 The inspection port and its fake (`domovoi_core::ports::HostInspect`, **fixed** behaviour, **convention** method names)

A synchronous, read-only trait: **it has no method that changes anything on the host.** That is what makes "the start check never repairs" structural, and the gate `helper_start_never_chmods_an_existing_directory` pins it (6.7).

```rust
pub trait HostInspect: Send + Sync {
    fn euid(&self) -> u32;
    fn egid(&self) -> u32;
    fn supplementary_groups(&self) -> Result<Vec<u32>, InspectError>;              // getgroups()
    fn user(&self, name: &str) -> Result<Option<UserEntry>, InspectError>;
    fn group(&self, name: &str) -> Result<Option<GroupEntry>, InspectError>;
    fn groups_of(&self, user: &UserEntry) -> Result<Vec<u32>, InspectError>;       // getgrouplist, NSS-aware
    fn lstat(&self, path: &Path) -> Result<PathFacts, InspectError>;               // no symlink follow
    fn open_facts(&self, path: &Path) -> Result<PathFacts, InspectError>;          // fstat of a descriptor opened O_NOFOLLOW
    fn list_dir(&self, path: &Path) -> Result<Vec<String>, InspectError>;
    fn walk_tree(&self, path: &Path) -> Result<Vec<TreeEntry>, InspectError>;      // no symlink follow; for the write-bit rule
    fn probe(&self, path: &Path, how: Probe) -> Access;                            // Probe::{Open, List, Connect}; Access::{Allowed, NotFound, Denied, ReadOnly, Other}
    fn read_text(&self, path: &Path) -> Result<String, InspectError>;              // /proc/self/status, os-release, lsm, mountinfo
    fn kernel_release(&self) -> Result<String, InspectError>;
    fn state_write_timings(&self, dir: &Path, cycles: u32) -> Result<Vec<u64>, InspectError>; // ST-20: microseconds per write, fsync, rename, directory fsync
}
```

`InspectError` is a `thiserror` enum (`NotFound`, `PermissionDenied`, `Io(String)`); an adapter never panics. The production adapter is `domovoi_helper::inspect::UnixHostInspect` (`nix` features already in the workspace: `user`, `fs`; no `unsafe`, no `libc`). `crates/domovoi-testkit/src/host_inspect.rs` provides `FakeHostInspect`: a builder holding accounts, groups, per-path `PathFacts`, per-path `Access` results, text files and timings; every call is recorded. Systemd queries go through the existing `FakeProcessExecutor`.

### 6.4 Self-test model (`domovoi_core::isolation`, pure; **fixed**)

```rust
pub enum CheckId { St01, St02, /* ... */ St20 }                 // as_str "ST-01" ... "ST-20"; CheckId::ALL has 20 entries
pub enum RunBy { Agent, Root, Runner }
pub enum Outcome { Pass, Fail { token: CheckToken, detail: String }, Warn { token: CheckToken, detail: String },
                   Info { detail: String }, Nd { reason: String } }   // Nd: not run here; never a pass, never satisfies a required check
pub struct CheckResult { pub id: CheckId, pub run_by: RunBy, pub outcome: Outcome, pub faults: Vec<PathFault> }
pub struct SelftestReport { pub results: Vec<CheckResult>, pub platform: PlatformId, pub systemd_version: Option<String>,
                            pub boundary: BoundaryState }         // serde: this is the JSON of `selftest --json`
```

| Id | Run by | What 47a does | Result tokens |
|---|---|---|---|
| ST-01 | agent | `euid` equals the uid of `domovoi-agent`, not 0 | `privilege_boundary_not_in_effect` |
| ST-02 | agent | `evaluate_agent_groups` over `getgroups()` | `agent_groups_unexpected` (with the gids) |
| ST-03 | root | `evaluate_skill_account` | `skill_account_invalid` |
| ST-04 | root | `evaluate_docker_membership` for both accounts | `docker_group_member` |
| ST-05 | runner | evaluator `evaluate_runner_groups` only; collection is 47b; `nd{runner_not_installed}` until the runner reports | `runner_groups_extra` |
| ST-06 | runner | evaluator over the text of `/proc/self/status` (`CapEff` and `CapBnd` zero, `NoNewPrivs` 1); collection 47b; `nd` until then | `runner_privileges` |
| ST-07 | root | `systemctl show` of the helper socket, `domovoi-helper@selftest.service`, the agent unit (6.10); runner entries `nd` | `unit_modified`, `unit_not_inspectable` |
| ST-08 | root | record systemd version string and build; compare nothing | `info` |
| ST-09 | root | no numeric uid or gid in a shipped unit; no `ExecStart=`, `ExecStartPre=` or `ExecStopPost=` path under `/run`, `/tmp` or `/dev/shm` | `unit_hardcoded_id`, `unit_exec_under_run` |
| ST-10 | root | `evaluate_path` for every `expected_paths` entry, plus the tree rule for `SkillsDir` and `PackagesDir`; reports **every** fault; never repairs | `path_mode_wrong` |
| ST-11 | root | helper directory, lock, key, state, platform record through `open_facts`; the lock opens | `helper_state_permissions` |
| ST-12 | agent or root | the helper socket node: socket type, `root:domovoi-agent`, 0660 (0666 fails); runner socket `nd` | `socket_mode_wrong` |
| ST-13 | runner | evaluator of the eight-point report (6.5) over a fabricated or collected `IsolationReport`; `nd{runner_probe_unavailable}` until 47b | `runner_not_isolated{what}` |
| ST-14 | agent | connect to the helper socket and send `health`; the runner socket sub-result is `nd` until 47b; the overall result is `nd` while any sub-result is `nd`, but the helper sub-result is reported separately and is what the helper read decision uses | `socket_unreachable` |
| ST-15 | agent | open `attest.key`, list `/var/lib/domovoi-helper`, open `state.lock`, read `/proc/1/environ` and (when it still exists) `/proc/<pid from health>/environ`: all `EACCES` | `agent_reaches_helper_state` |
| ST-16 | agent | connect to the Docker socket (`/var/run/docker.sock`): `EACCES` or absent | `agent_reaches_docker` |
| ST-17 | root | container installs only: no read-write `docker.sock` bind, not privileged, no bind of `/` or of the helper directory; `nd{not_a_container_install}` on a host install | `container_escape_surface` |
| ST-18 | root | `/etc/domovoi` is not an empty `0755` directory created by a mask | `host_directory_created_by_mask` |
| ST-19 | root | record `/sys/kernel/security/lsm`, Yama `ptrace_scope`, the two `apparmor_restrict_unprivileged_userns*` values, mount options of `/run` and `/tmp`; the AppArmor label of the probe instance is 47b | `info` |
| ST-20 | root | ten write cycles on the state filesystem; median above 100 000 microseconds | `state_write_slow` (warn, never blocks) |

**Tolerant forms (synthesis 02 section 5.9.1; the first wordings misclassified a good host on systemd 255 and must not be implemented).**

- **ST-07.** Query `<template>@selftest.service` (the bare template name is rejected by every measured systemd). The instance string `selftest` is a constant in `selftest/units.rs`, the one place `ID-3` allows. Read `LoadState`, `FragmentPath`, `DropInPaths`, `NeedDaemonReload`, `Accept`, `SocketMode`, `SocketUser`, `SocketGroup`, `Type`, `User`, `RuntimeMaxUSec` with `systemctl show --no-pager --property=...`. Compare `RuntimeMaxUSec` after **parsing the duration text** (`6s`, `1min`, `infinity`, `1min 30s` all occur). A non-zero exit of `systemctl show`, or an empty `FragmentPath`, is `unit_not_inspectable`, never a skip and never a pass. A non-empty `DropInPaths` is `unit_modified`, also before any `daemon-reload`; `NeedDaemonReload=yes` is `unit_modified`. `systemd-analyze verify` is not a check.
- **ST-13 point 3** (`/run`): `listdir('/run')` is a subset of `['systemd']`; `listdir('/run/systemd')` is a subset of `['incoming']`; if `incoming` exists it is a directory, uid 0, `mode & 0o077 == 0`. An empty `/run` passes. Anything else, in particular a socket, fails.
- **ST-13 point 4** (writes): create, rename and unlink in `/etc/domovoi/skills` and `/var/lib/domovoi-packages` give exactly `EROFS`; append to an existing file never succeeds and gives `EROFS` or `EACCES`; point 7 requires the `ro` option on both binds and on the three masks.

### 6.5 The eight-point isolation report and its evaluator (`isolation/probe.rs`, `isolation/evaluate.rs`; **fixed** points, **convention** field names)

`IsolationReport` is plain data collected by the runner in probe mode (47b) through `domovoi_helper::probe::collect(&dyn HostInspect, &ProbeArgs { helper_pid })` (shipped and tested here with `FakeHostInspect`). `evaluate_isolation(&IsolationReport) -> Vec<IsolationFault { point: u8, what: String }>` returns the failing points. The eight points:

1. `connect()` to both socket paths gives `ENOENT` (the mask), not `EACCES`.
2. `stat`, `listdir` and `open` of `/var/lib/domovoi-agent`, `/var/lib/domovoi-helper` and `attest.key` give `ENOENT`.
3. `listdir('/etc/domovoi')` is `['skills']`; `listdir('/var/lib')` is `['domovoi-packages']`; `helper.toml` and `agent.toml` give `ENOENT`; `/run` as in the tolerant form above.
4. Writes as in the tolerant form above.
5. `/home` and `/root` give `EACCES` or are absent; `/run/user` is absent.
6. `/proc/<helper pid>/` and `/proc/1/environ` give `ENOENT`.
7. Group list, `CapEff`, `CapBnd`, `NoNewPrivs`, uid, gid, AppArmor label (informational) and the mount points of the three masks from `/proc/self/mountinfo`, `ro` required on both binds and on the three masks.
8. `/etc/shadow` gives `EACCES` (informational only).

`EACCES` where `ENOENT` is expected is a **failed mask**, never a pass: the evaluator tells them apart (`isolation_report_distinguishes_eacces_from_enoent`).

### 6.6 `helper.toml` (`domovoi_config::HelperConfig`, **fixed** keys; follow `AgentConfig`'s loader and `deny_unknown_fields`)

```toml
# /etc/domovoi/helper.toml (root:root 0600). Written by the installer only when absent.
groups = ["docker.read", "systemd.read", "smart.read", "kernel.read", "net.read", "safety.attest"]

[platform]
allow_write_groups_on_untested = false     # the per-host opt-in (D-39): off by default, owner-edited

[smart]
devices = ["/dev/sd[a-z]", "/dev/nvme[0-9]n1"]

[safety]                                    # parsed, not interpreted in 47a (45, 46, 47c)
management_cidrs = []
protected_units = ["sshd.service", "domovoi-agent.service", "domovoi-helper.socket", "domovoi-skill-runner.socket"]
protected_containers = []

[safety.containment]
max_ttl = "1h"
max_active = 0

[limits]
timeout = "30s"
mutations_per_hour = 0
```

`[docker]`, `[systemd]` and `[nft]` sections from `04` §8.3 are accepted with their documented keys and not interpreted. An unknown key, an unknown group name or a duplicate group is a start-time `helper_config_invalid` (a config error, like the agent's). `HelperConfig::effective_groups()` returns the configured groups minus every write group; the removed ones are reported in `Groups.ignored`. The default (no file, or an empty `groups`) is the fresh-install set above; **no write group is ever enabled by default**.

### 6.7 The helper crate (`crates/domovoi-helper`, **fixed** behaviour)

Dependencies: `domovoi-core`, `domovoi-config`, `async-trait`, `serde`, `serde_json`, `thiserror`, `tokio` (`rt`, `net`, `io-util`, `time`, `macros`), `tracing`, `uuid`, `jiff`, and `nix` (workspace entry, features `user`, `fs`) on Unix. Nothing else; no new workspace dependency. `[lints] workspace = true`; the crate root has a `//!` doc (R10). It spawns no process in this prompt (the series rule is in section 5).

**`adopt` (D-40, synthesis 05 section 6).**

```rust
pub enum AdoptError { UnexpectedListenFds(String), NotASocket(std::io::Error), Io(std::io::Error) }
pub fn check_listen_env(listen_fds: Option<&str>) -> Result<(), AdoptError>;                 // pure: None or "0" pass; anything else (including malformed) fails
pub fn adopt_connection(fd: std::os::fd::BorrowedFd<'_>) -> Result<std::os::unix::net::UnixStream, AdoptError>; // try_clone_to_owned (F_DUPFD_CLOEXEC), local_addr() validates (ENOTSOCK for a tty, file or pipe)
pub fn adopt_stdin_connection() -> Result<tokio::net::UnixStream, AdoptError>;               // reads LISTEN_FDS, borrows Stdin, set_nonblocking, from_std
```

No `unsafe`; `LISTEN_*` is never unset (edition 2024 forbids it safely) and never used to find a descriptor; nobody reads through `std::io::stdin()` afterwards. `LISTEN_PID` is irrelevant on this path. `Accept=no` and fd 3 are unsupported: `listenfd` is the documented fallback and is **not** added.

**`serve`.**

```rust
pub trait Connection: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send { fn peer_uid(&self) -> std::io::Result<u32>; }
impl Connection for tokio::net::UnixStream { /* peer_cred().uid() */ }
pub struct ServeContext { pub config: Result<HelperConfig, ConfigError>, pub inspect: Arc<dyn HostInspect>, pub clock: Arc<dyn Clock>,
                          pub read_deadline: SignedDuration /* 10 s */, pub build: String }
pub async fn serve_connection<C: Connection>(conn: C, ctx: &ServeContext) -> ServeOutcome;   // never panics, never returns an error the process maps to a non-zero exit for a handled request
```

Order, fixed: (1) `SO_PEERCRED`: the peer uid must equal the uid of `domovoi-agent` resolved by name **at this moment**; otherwise write `Refused{peer_not_agent}` and close, revealing nothing else (root is **not** accepted; root diagnostics use `selftest`, not the socket); (2) read the 4-byte length, enforce `MAX_FRAME_BYTES` before reading the body, read the body under `read_deadline` measured on the injected `Clock` (the agent's `within` pattern; no `tokio::time`); (3) run the start checks; a failure answers **every** request with `StartRefused{token, faults}` after the peer check (the faults are the same list the operator sees) and logs them at error level; (4) decode (`decode_request`); (5) dispatch (6.2): `Health`, `Groups`, `Read` and `SafetyAttest` as specified, the last two always `Unavailable` (`GroupDisabled` when the group is not in `effective_groups()`, otherwise `NotImplemented`; write groups are `WriteGroupNeedsOptIn` or `PlatformBlocked` per 6.9 when ever asked); (6) write one frame, shut the stream down, return; the subcommand exits 0. A defect or I/O error exits non-zero and leaves a `failed` instance for `reset-failed` (F-CAN-3).

**`start_check` (D-38, synthesis 02 section 5.10).**

```rust
pub struct StartRefusal { pub token: RefusalToken, pub faults: Vec<PathFault> }
pub fn run_start_checks(inspect: &dyn HostInspect, dirs: &AgentDirs) -> Result<(), StartRefusal>;
```

It resolves `domovoi-agent` (absent: `agent_account_missing`); `lstat`s `HelperDir` (absent: `uninitialized`); checks `HelperDir`, `HelperLock` (through `open_facts`, so an unopenable lock is `helper_state_permissions`, **never** `state_corrupt`), and `HelperKey`, `HelperState`, `HelperPlatform` **when present**, plus `HelperToml`; collects **every** fault (found and expected mode and owner and group, kind) and returns `helper_state_permissions`, or `helper_config_invalid` when the file parses wrong. It **only reads**: it uses `HostInspect` and nothing that changes a mode or an owner, and the module contains no call to `set_permissions`, `chmod`, `chown`, `fchmod`, `fchown` or `create_dir` (gate, section 12). In 47a the absence of `attest.key` and `state` does **not** refuse start (no 47a request needs them); 47c adds their presence and checksum checks for `safety.attest` only.

**`state`.**

```rust
pub struct StateLayout { pub root_uid: u32, pub root_gid: u32 }              // production (0, 0); tests pass the current user
pub fn init(layout: StateLayout, dir: &Path) -> Result<InitOutcome, InitError>;
//   creates `dir` with mode 0700 (DirBuilder::mode, then verified) and `state.lock` (O_CREAT|O_EXCL|O_NOFOLLOW, 0600);
//   an existing directory or lock with the expected facts is `AlreadyInitialised` (idempotent); with any other facts it is
//   refused with the fault list and NOTHING is changed (never accepts an existing 0755 directory, F-CUS-5). No key, no state file: 47c.
pub struct StateLock { /* file opened O_NOFOLLOW, regular, nix flock */ }
impl StateLock { pub fn open(dir: &Path) -> Result<Self, LockError>;
                 pub fn try_acquire(&self) -> Result<Option<LockGuard<'_>>, LockError>;   // LOCK_EX|LOCK_NB; None = EWOULDBLOCK
                 pub fn acquire(&self) -> Result<LockGuard<'_>, LockError>; }             // blocking LOCK_EX
pub struct WriteFaults { pub temp_sync: bool, pub rename: bool, pub dir_sync: bool }       // test fault injection, default all false
pub fn write_atomic_locked(guard: &LockGuard<'_>, dir: &Path, name: &str, bytes: &[u8], mode: u32, faults: WriteFaults)
    -> Result<(), StateWriteError>;
//   a leftover `.<name>.tmp` is removed under the lock; the temp file is created O_EXCL|O_NOFOLLOW with `mode`; write, fsync,
//   rename over `name`, fsync of the directory. Requiring `&LockGuard` makes "inside the lock" a type fact.
//   `StateWriteError::{Failed, DurabilityUnknown}` separates "nothing changed" from "renamed but the directory fsync failed".
```

The write helper is used in 47a by `selftest --record` (6.9) and is the one 47c uses for the state file. Time is never read inside it.

**`repair` (D-11; the command is `domovoi helper repair`, name decided here).**

```rust
pub struct RepairEntry { pub path: PathBuf, pub found: PathFacts, pub expected: PathFacts, pub change: Change /* Chmod(u32) | Chown{uid,gid} | Both */ }
pub struct RepairPlan { pub entries: Vec<RepairEntry>, pub refused: Vec<(PathBuf, RefuseReason)> }   // Symlink | WrongType | Missing
pub fn plan_repair(faults: &[PathFault]) -> RepairPlan;                         // pure
pub trait Confirm { fn confirmed(&self, plan: &RepairPlan) -> bool; }           // production: prompts on a terminal, accepts only the typed word `repair`
pub fn apply_repair(plan: &RepairPlan, confirm: &dyn Confirm, ops: &dyn RepairOps) -> RepairOutcome;
```

It must: run only as root; print the plan (each path, found and expected mode, owner and group, the exact change); refuse and change nothing when stdin is not a terminal (`confirmation_requires_terminal`: there is **no** `--yes`); refuse a symlink or a path of the wrong type; change **only** listed paths and only mode and ownership (never content, never create, never delete); re-run ST-10 and ST-11 afterwards and print what is left. It covers the faults of ST-10 and ST-11 only. It is invoked from `crates/domovoi/src/cmd/helper.rs` and nowhere else (gate `helper_repair_is_not_reachable_from_start_agent_hub_or_chat`).

### 6.8 Agent side (`domovoi-agent`, **fixed** behaviour)

```rust
// helper_client.rs. The only agent module that names HELPER_SOCKET or builds a HelperRequest (HELPER-1).
pub struct HelperClient { /* socket path */ }
impl HelperClient { pub fn new(socket: impl Into<PathBuf>) -> Self; pub fn default_path() -> Self;
    pub async fn health(&self, clock: &dyn Clock) -> Result<HealthInfo, HelperClientError>;
    pub async fn groups(&self, clock: &dyn Clock) -> Result<GroupsInfo, HelperClientError>; }
pub async fn probe_status(client: &HelperClient, clock: &dyn Clock) -> HelperStatus;
//   socket path missing (ENOENT)            -> HelperStatus::Absent
//   connect or frame failure, Refused, StartRefused -> HelperStatus::Error { reason: <token or "unreachable"> }
//   health and groups answered              -> HelperStatus::Ok { ops: <enabled group names, "docker.read", ...> }
```

`runtime.rs` runs `probe_status` at start and then every `STATUS_PROBE_INTERVAL` (60 s, `link/mod.rs:61`) on the injected `Clock`, writing the result into the existing `AgentStatusSource`; the link's existing change probe then sends `agent_status` on a change. A helper that starts after the agent therefore turns `Absent` into `Ok` within one interval. `helper_identity` stays `None` (47c). `skill_runner_status` stays `Absent` (47b).

```rust
// state_owner.rs
pub struct StateOwner { pub uid: u32, pub gid: u32 }
pub fn resolve_agent_owner() -> Result<Option<StateOwner>, StateOwnerError>;    // Some only when euid == 0 and the account exists
pub fn hand_over(dir: &Path, owner: StateOwner) -> Result<HandOverReport, StateOwnerError>;
//   walks `dir` without following symlinks (a symlink anywhere is refused), `fchownat(AT_SYMLINK_NOFOLLOW)` each entry whose owner
//   differs, never changes a mode, reports counts. Idempotent.
```

`domovoi agent enroll` and `agent journal init|reset` run as root with `sudo` (`docs/book/src/install/binary.md:46`) and would leave root-owned files that the unprivileged agent cannot read. When they run as root and `resolve_agent_owner()` is `Some`, they call `hand_over(state_dir, owner)` after their last write and print one line saying so. `agent.toml` stays `root:root`; its two write sites (`config_file.rs:77`, `:127`) change from `0o640` to `0o644` on Unix. `PackageDeps` receives `boundary_in_effect: bool` (from `BoundaryState`, evaluated once at agent start with ST-01 and ST-02 over a `HostInspect`) and `privilege_boundary()` (`packages/mod.rs:262`) returns `InEffect` only when it is true; `runs_as_root` keeps its meaning and its wire field. `BoundaryState` is `InEffect` or `NotInEffect { reason: AgentIsRoot | AgentNotDomovoiAgent{uid} | AgentGroupsUnexpected{gids} | AccountMissing }`.

### 6.9 Platform tier and the per-host opt-in (`isolation/tier.rs`, `domovoi-helper/src/platform.rs`; **fixed** behaviour, **convention** wording)

```rust
pub enum PlatformTier { Supported, Untested, Blocked }
pub struct PlatformId { pub os_id: String, pub version_id: String, pub arch: String }     // from /etc/os-release ID and VERSION_ID, and std::env::consts::ARCH
pub const SUPPORTED_PLATFORMS: &[PlatformId] = &[];                                       // nothing is release-tested (synthesis 00 section 5b)
pub const REQUIRED_CHECKS: &[CheckId] = &[St01, St02, St03, St04, St05, St06, St07, St09, St10, St11, St12, St13, St14, St15, St16, St18];
pub struct WriteOptIn(pub bool);                                                          // from [platform] allow_write_groups_on_untested
pub struct TierDecision { pub tier: PlatformTier, pub skills: SkillsDecision, pub helper_read: ReadDecision, pub helper_write: WriteDecision,
                          pub notice: Option<TierNotice>, pub blocking: Vec<CheckId> }
pub fn classify_platform(report: &SelftestReport, supported: &[PlatformId], opt_in: WriteOptIn) -> TierDecision;
```

Rules, fixed: any `Fail` in a required check, or `Nd` in a required check, or no systemd, gives `Blocked` and lists the ids in `blocking`; otherwise a platform in `supported` is `Supported` and any other is `Untested`. `skills`: `Allow` (Supported), `AllowWithNotice` (Untested), `Refuse` (Blocked). `helper_read`: allowed when ST-10 to ST-12 (helper sub-results) and ST-15 pass and the start checks pass, whatever the tier. `helper_write`: `Allow` (Supported), `NeedsOptIn` or `Allow` (Untested, by `WriteOptIn`), `Refuse` (Blocked). A `Warn` never changes the tier (ST-20). `TierNotice::UntestedPlatform` carries the text (convention; the owner may amend before merge): "This platform has not been release-tested. Skills run here after you acknowledge that; privileged write operations stay off unless you enable them for this host in helper.toml." The display and the recorded acknowledgement are consumers' work (47b, hub).

**The helper's view of the tier.** The helper never trusts a tier a caller claims. `domovoi agent selftest --installer --record` (root only) writes `/var/lib/domovoi-helper/platform.json` (`{format, recorded_at, kernel_release, platform, tier, blocking}`, mode 0600, through `write_atomic_locked`, `recorded_at` from the `Clock`). The helper reads it in `Groups`: `PlatformRecordState::{Ok, Missing, Stale}` (`Stale` when `kernel_release` differs from the running kernel). `Missing` and `Stale` give `helper_write = Refuse` (tokens `platform_record_missing`, `platform_record_stale`). The opt-in is `[platform] allow_write_groups_on_untested` and `Groups.write_opt_in` reports it, so that the agent can relay it and the hub can audit it; the hub-side audit row (`helper.write_opt_in`) is added by the first prompt that has a write group (45). In 47a a switch that is turned on is logged at warn level by the helper at every start (the journal is the local record) and appears in `Groups`.

### 6.10 Commands (**fixed** names and behaviour, **convention** flag spelling beyond these)

- `domovoi helper serve`: adopt fd 0, run `serve_connection` once, exit 0 (error: non-zero). Never runs interactively (refuses when fd 0 is not a socket: `adopt_not_a_socket`).
- `domovoi helper init`: root only; `state::init`; prints `created` or `already initialised`; non-zero with the fault list on a wrong existing layout.
- `domovoi helper repair`: 6.7.
- `domovoi agent selftest [--installer] [--record] [--json] [--config <agent.toml>]` and the hidden `--agent-checks` (the agent-run subset as JSON, used by the installer). Without `--installer` it runs the agent-side checks (ST-01, ST-02, ST-12, ST-14, ST-15, ST-16) and, as far as the account may, reports the rest as `nd{needs_root}`. With `--installer` (root) it runs the root checks itself and obtains the agent-run results by **spawning** `<current exe> agent selftest --agent-checks --json` through the injected `ProcessExecutor` with `run_as = Some("domovoi-agent")` (the existing `ProcessSpec.run_as` mechanism; no new spawn site), then merges both lists. Output: one line per check (`ST-10 fail path_mode_wrong ...`, then each fault with found and expected values), then the boundary label and the tier decision; `--json` prints the `SelftestReport`. Exit 0 when no check failed, 1 when any failed. `nd` is printed, never counted as pass. The command also lists skills whose manifest `run_as` is not `domovoi-skill` (D-31's "selftest lists affected skills"), reading manifests through the existing catalog loader; enforcement is 47b.
- The top-level `domovoi selftest` is unchanged.

### 6.11 Units (**fixed** text; `deploy/systemd/`, root-owned 0644, every `ExecStart=` outside `/run`)

`deploy/systemd/domovoi-helper.socket`:

```ini
[Unit]
Description=Domovoi helper socket

[Socket]
ListenStream=/run/domovoi-helper.sock
SocketUser=root
SocketGroup=domovoi-agent
SocketMode=0660
Accept=yes
MaxConnections=8
RemoveOnStop=yes

[Install]
WantedBy=sockets.target
```

`deploy/systemd/domovoi-helper@.service` (the measured baseline of synthesis 02 section 5.3, unchanged; `/usr/local/bin/domovoi` is rewritten to the install prefix by the installer, like the other units):

```ini
[Unit]
Description=Domovoi helper (connection %i)

[Service]
Type=simple
# Root: no User= line on purpose.
StandardInput=socket
StandardOutput=journal
StandardError=journal
ExecStart=/usr/local/bin/domovoi helper serve
Restart=no
NoNewPrivileges=yes
ProtectSystem=strict
ReadWritePaths=/var/lib/domovoi-helper
ProtectHome=yes
PrivateTmp=yes
PrivateDevices=yes
CapabilityBoundingSet=CAP_NET_ADMIN
AmbientCapabilities=
RuntimeMaxSec=60s
KillMode=control-group
TimeoutStopSec=5s
```

| Directive | Reason and evidence |
|---|---|
| `SocketUser=root`, `SocketGroup=domovoi-agent`, `SocketMode=0660` | first layer; a client outside the group is refused before reaching the service on every run (F-ACT-4); `0660` is a hard maximum, a `0666` socket is not protected by any directive (F-MSK-13) |
| `Accept=yes`, `MaxConnections=8` | one process per connection (D-36); the lock is held across a request, so instances serialise and no in-memory state outlives a request; `8` loaded as written, behaviour at the limit unmeasured |
| `RemoveOnStop=yes` | the node otherwise survives a stop (measured) |
| `Type=simple` | `RuntimeMaxSec` works with it and is ignored with `oneshot` (F-CAN-4, F-CAN-5) |
| no `User=` | the helper is root (RW-39); the peer uid check is its first layer, not the isolation |
| `StandardInput=socket`, `StandardOutput=journal`, `StandardError=journal` | the connection is fd 0 and carries the frames; stdout is not the connection (F-ACT-2) |
| `ExecStart=` outside `/run` | `/run` may be `noexec`; direct exec under it fails `203/EXEC` (UNIT-9) |
| `NoNewPrivileges=yes`, `ProtectSystem=strict`, `ProtectHome=yes`, `PrivateTmp=yes`, `PrivateDevices=yes` | measured shape of the root stand-in (`/etc` and `/var/lib` `EROFS`, key readable); `strict` leaves `/home` writable, so `ProtectHome` is required (F-PRT-2) |
| `ReadWritePaths=/var/lib/domovoi-helper` | the only writable path; a plain path that is absent **fails the start** (fail closed), so the installer creates it before the socket starts (INSTALL-1) |
| `CapabilityBoundingSet=CAP_NET_ADMIN`, `AmbientCapabilities=` | the value measured (`CapEff=CapBnd=0x1000`); the read groups and `safety.attest` need none of it; the baseline shares one unit; narrowing needs one unit per group or an in-process drop, neither measured (B5), so it is left to the first prompt that adds a group that needs it (47d, which widens the shared baseline once as its decision OD-1: 47a owns the unit file and ships and tests the baseline text above, 47d amends it with a clearly marked candidate delta and updates `UNIT-3`) |
| `RuntimeMaxSec=60s`, `KillMode=control-group`, `TimeoutStopSec=5s` | the time bound of one request; written out so a gate can assert it |

No `ProtectKernel*`, `RestrictAddressFamilies`, `RestrictNamespaces`, `LockPersonality` or `SystemCallArchitectures` line ships: synthesis 02 lists them as **unmeasured candidates**, and `RestrictNamespaces=yes` conflicts with the containment leak self-test (0031 Decision 5).

`deploy/systemd/domovoi-agent.service` changes (and only these): the comment block no longer says "It runs as root"; `User=domovoi-agent`; `Group=domovoi-agent`; `Wants=` and `After=` gain `domovoi-helper.socket` (the agent works without the helper, so `Wants`, not `Requires`); `ReadWritePaths=/var/lib/domovoi-agent /var/lib/domovoi-packages`. `RestrictSUIDSGID=no`, `ProtectSystem=full` and the other lines stay (47b revisits them when skills leave the agent). There is no `SupplementaryGroups=` line: the account carries `systemd-journal`. `install.sh` embeds all three new texts (`hub_unit`, `agent_unit`, and two new functions), `--print-unit` accepts `helper-socket` and `helper-service`, and `deploy/test/installer.sh` compares each embedded text with its file.

### 6.12 Arch rules (`xtask/src/arch/{units,install_script,identity_rules}.rs`; **fixed** ids and meaning)

The unit parser reads `deploy/systemd/*.service|*.socket` as ordered `(section, key, value)` triples (repeated keys allowed, `\` continuations joined, `;`/`#` comments ignored). The install-script parser reads `install.sh` as lines (heredoc bodies of the embedded units are skipped). Each rule is implemented with a card (`rules.rs` data) and a unit test that feeds it a failing input; `ARCHITECTURE.md` §4 and §15 get a bullet. `rules.rs` changes need the ADR (`arch-check --base`).

| Rule | Checks | Backs |
|---|---|---|
| `HELPER-1` | `HELPER_SOCKET`, `HelperRequest`, `HelperRequestFrame`, `HelperClient` and the literal `domovoi-helper.sock` appear only in `domovoi-core/src/helper/`, `domovoi-helper/`, `domovoi-agent/src/helper_client.rs`, `domovoi-agent/src/runtime.rs` (the probe call only), `domovoi/src/cmd/helper.rs`, `domovoi-testkit/`, test paths and `deploy/`; never in the hub, engine, store, llm, chat, checks, config, proto, runner or the rest of the agent | 0030 Decision 8 |
| `HELPER-2` | `/var/lib/domovoi-helper`, `attest.key`, `state.lock` and `platform.json` appear only in `domovoi-helper/`, `domovoi-core/src/identity/paths.rs`, test paths, `deploy/`, `install.sh` and docs | 0030 Decision 8 |
| `IDENT-1` | the exact string literal `"domovoi-skill"` appears in non-test Rust only in `domovoi-core/src/identity/accounts.rs` (the `run_as` enforcement itself is 47b) | D-31 |
| `ID-2` | the literals `"domovoi-agent"` and `"domovoi-skill"` and the group-membership calls (`groups_of`, `supplementary_groups`) live in `domovoi-core/src/identity/` and `domovoi-helper/src/{inspect,selftest,start_check}`; no numeric uid or gid other than 0 is assigned within three lines of an account constant | F-DOC-7 |
| `ID-3` | in `domovoi-helper`, `domovoi-agent/src/helper_client.rs` and `domovoi-runner`, no `split('@')`, `rsplit('@')`, `split_once('@')`, `rsplit_once('@')` or comparison of a systemd instance string; the one allowed instance literal is `SELFTEST_INSTANCE` in `selftest/units.rs` | 02 section 2.1.1 |
| `UNIT-2` (docker clause) | no `Group=` or `SupplementaryGroups=` value in any shipped unit contains `docker` (the runner clauses are 47b) | F-GRP-1, F-DOC-4 |
| `UNIT-3` (helper) | `domovoi-helper@.service` has each directive of 6.11 with its value, has no `User=` and no `SupplementaryGroups=` (the runner unit joins the same table in 47b) | F-PRT-2, F-ACT-2 |
| `UNIT-4` | a unit with `RuntimeMaxSec=` has `Type=simple` | F-CAN-5 |
| `UNIT-5` | every shipped `.socket` has `SocketMode=0660`, `SocketUser=root`, `SocketGroup=domovoi-agent` and a `ListenStream` that is one of the two literals (the second belongs to 47b) | F-ACT-4, F-MSK-13 |
| `UNIT-7` | every path in a plain `InaccessiblePaths=`, `BindReadOnlyPaths=`, `TemporaryFileSystem=` or `ReadWritePaths=` is in the installer's created-set (`CREATED_PATHS`, one list between `# BEGIN created-set` and `# END created-set` in `install.sh`) or is exactly `/var/lib` or `/run`; a `-` prefix on a created-set path fails | F-MSK-7, F-MSK-8, F-MSK-10 |
| `UNIT-8` | `domovoi-agent.service` has `User=domovoi-agent`; no `volumes:` entry under `deploy/compose/` that names `docker.sock` is read-write (the socket-proxy entry is `:ro` today) | D-29, F-DOC-6 |
| `UNIT-9` | no `ExecStart=`, `ExecStartPre=` or `ExecStopPost=` of a shipped unit has a path under `/run`, `/tmp` or `/dev/shm` | U section 3.3 |
| `INSTALL-1` | in `install.sh`, the line that creates each `CREATED_PATHS` entry precedes the first `systemctl enable` or `systemctl start` | F-MSK-10 |
| `SELFTEST-1` | every `CheckId`, every `CheckToken` and every `RefusalToken` named in an `ALL` constant appears in a `// covers: <id or token>` comment above a test in `domovoi-helper` or `domovoi-core` | all |

`UNIT-1` and `UNIT-6` (the runner's masks and binds) are 47b's. The existing `R1` table gets the new crate in `CRATES`: internal `domovoi-core`, `domovoi-config`; external allowlist `async-trait`, `serde`, `serde_json`, `thiserror`, `tokio`, `tracing`, `uuid`, `jiff`, `nix`. `domovoi` gains `domovoi-helper` in its internal list. A gate in `xtask`'s tests (`shipped_units_pass_every_unit_rule`) runs all unit rules over the real `deploy/` tree.

## 7. Ordered implementation steps

1. **Inspect and verify; report differences.** Open every file, symbol and line in section 3. Confirm 44 is merged and ADRs 0030 to 0032 carry the research updates; confirm there is no `domovoi-helper` crate; list `docs/adr/` for the next number; confirm `crates/domovoi-agent/src/packages/` and the `PrivilegeBoundary` code are as described. Write every difference down before editing; stop if one changes the design (for example if `Clock::sleep_until` or `ProcessSpec.run_as` changed, or if the agent no longer reads `agent.toml` through `/etc/domovoi`).
2. **ADR, crate shell and rules data.** Write the ADR (`docs/adr/0033-helper-crate-and-isolation-rules.md`, or the next free number; A2: the crate and its edges; the rule list; the decision that the helper spawns nothing; status `proposed`, extends 0030 and 0031). Add the crate (`Cargo.toml`, empty `lib.rs` with its `//!`), the `[workspace.dependencies]` path entry, the `CRATES` row and the `domovoi` edge in `rules.rs`, and the `ARCHITECTURE.md` §4 row. `cargo xtask arch-check` and `--base origin/overhaul-investigation` pass.
3. **Core** (6.1 to 6.5): `identity`, `helper`, `isolation`, the `HostInspect` port, all pure evaluators, `classify_platform`, the tokens with their `ALL` constants. Unit tests for every evaluator and every tolerant form, with the negative cases of 6.4.
4. **Testkit:** `FakeHostInspect`.
5. **Config** (6.6): `HelperConfig`, validation, the fresh-install default, tests.
6. **Helper crate, runtime half:** `adopt`, `state` (lock, atomic, init), `start_check`, the Unix `HostInspect` adapter, `serve`. Tests over `UnixStream::pair()` and tempdirs.
7. **Helper crate, checks half:** `selftest` (agent and root checks, `units.rs` with the duration parser, rendering, `--record`), `probe::collect`, `platform`, `repair`. Tests with `FakeHostInspect` and `FakeProcessExecutor`.
8. **Agent:** `helper_client`, the status task in `runtime.rs`, `state_owner` and its three call sites, `config_file.rs` modes, `boundary_in_effect` into the packages service. Agent tests with `FakeClock` and a stub helper over a socketpair.
9. **Binary:** `cmd/helper.rs`, `agent selftest` in `cmd/agent.rs`, the production wiring (`SandboxExecutor`, `UnixHostInspect`, `SystemClock`). CLI tests (`helper_cli.rs`).
10. **Runner test:** add `spawned_skill_gets_no_listen_vars_and_no_connection` to `crates/domovoi-runner/tests/runner/exec.rs` (no runner code changes).
11. **Units, installer, scripts, CI:** the two unit files, the agent unit change, `install.sh` (accounts, directories, `helper init`, units, `helper.toml`, the helper socket, the selftest step, `--migrate-identity`, `--print-unit`), `deploy/test/installer.sh` additions, `deploy/test/identities.sh`, the CI classifier outputs and the `privileged` job (section 12).
12. **Arch rules and gates** (6.12): parsers, rules, cards, unit tests, the gate tests; run `cargo xtask arch-check` and with `--base`.
13. **Docs** (section 15), then the scoped checks of `AGENTS.md`, `deploy/test/installer.sh` where Docker is available, `deploy/test/identities.sh` on a root-capable host, and the completion report (the hosts and results of `identities.sh` go into it).

## 8. Database and persistence

No database change: no migration, no hub table, no store code. The hub is not touched.

Persistent state added on a host, all root-owned unless stated, all in the path table of 6.1:

| Item | Written by | Notes |
|---|---|---|
| `/var/lib/domovoi-helper/` (0700) and `state.lock` (0600) | `domovoi helper init` (installer step 3) | no key and no state file in 47a; 47c adds `attest.key` and `state` here, through `write_atomic_locked` |
| `/var/lib/domovoi-helper/platform.json` (0600) | `domovoi agent selftest --installer --record` | the helper's only view of the tier (6.9); atomic, under the lock |
| `/etc/domovoi/helper.toml` (0600) | the installer, only when absent | fresh-install defaults of 6.6 |
| `/etc/domovoi/identity-migration/` (0700) | `install.sh --migrate-identity` | the saved legacy unit and the recorded owners and modes, for rollback (section 14); never deleted automatically |
| `/var/lib/domovoi-packages` (0755 `domovoi-agent`) | the installer | the unprivileged agent cannot create it under `/var/lib` |

Atomic write and lock rules (`0031` Decision 6, `0032` Decision 9; the helper crate implements them, 47c reuses them): temp file in the same directory created exclusively with the final mode, `fsync`, `rename`, `fsync` of the directory, all inside `flock(LOCK_EX)` on `state.lock`; a leftover temp file is deleted under the lock; a second helper process gets `EWOULDBLOCK` on `try_acquire` while the lock is held. Durability across power loss is **not** demonstrated (recorded as residual in the ADR); ST-20 reports the measured write latency and never blocks.

## 9. Protocol and compatibility

- **The agent-hub wire is unchanged.** `crates/domovoi-proto/` is not touched; wire 1.2 and its golden frames stay as they are (R14). `helper_status`, `runs_as_root`, `skill_runner_status` and `helper_identity` are existing `hello` and `agent_status` fields. After 47a an agent reports `helper_status` as `Absent`, `Ok{ops}` or `Error{reason}` from a real probe (`ops` are group names such as `docker.read`); older hubs ignore nothing they did not already accept.
- **The helper socket protocol is new and local** (6.2): protocol number 1, one request and one response per connection, length-prefixed JSON, unknown fields and versions refused with a token. It is not exposed to the hub. A breaking change of it later bumps `HELPER_PROTOCOL`; the agent's client refuses a helper that answers another protocol with `Error{reason: "protocol"}`.
- **Version skew.** A new agent with no helper installed reports `Absent` and works as before. An old agent against a new helper never connects. A new helper with an old agent binary cannot happen (one binary).

## 10. Error handling and recovery

**Tokens (closed; `as_str` is the stable spelling; each has a named test).** Sources: synthesis 02 §5.9 and §5.10 and ADR 0032 E2. Tokens marked *(new)* are introduced by this brief for 47a-only conditions.

| Token | Where | Meaning and effect |
|---|---|---|
| `helper_state_permissions` | start check, ST-10 (helper paths), ST-11, `repair` | a wrong mode, owner, group, type or a symlink on the helper directory, lock, key, state, platform record or `helper.toml`; **refuse, never repair at start**; every offending path with found and expected values is reported; distinct from `state_corrupt` (an unopenable lock reports this token) |
| `uninitialized` | start check | `helper init` has not run (no directory or no lock); in 47a it refuses start; 47c extends it to "no key and no state" for `safety.attest` |
| `agent_account_missing` *(new)* | start check, ST-03 | the `domovoi-agent` account cannot be resolved; the helper cannot know whom to accept; refuse |
| `helper_config_invalid` *(new)* | start check | `helper.toml` does not parse (unknown key, unknown or duplicate group); refuse with the parse issues |
| `peer_not_agent` *(new)* | serve | `SO_PEERCRED` uid is not the uid of `domovoi-agent` (root is not accepted); the answer carries nothing else |
| `frame_too_large`, `frame_malformed`, `unsupported_version`, `unknown_operation` *(new)* | serve | the length exceeds 65 536, the body is not a request, `v` is not 1, the `op` string is not in the closed set |
| `adopt_unexpected_listen_fds`, `adopt_not_a_socket` *(new)* | `domovoi helper serve` | `LISTEN_FDS` is set and not `0`; fd 0 is not a socket; the process exits non-zero (a defect of the unit, not a handled request) |
| `platform_record_missing`, `platform_record_stale` *(new)* | `Groups` | write groups refused until `selftest --installer --record` runs on this kernel |
| `confirmation_requires_terminal` *(new)* | `repair` | stdin is not a terminal; nothing changed |
| `privilege_boundary_not_in_effect` | ST-01, label | the agent is not the `domovoi-agent` account (root, or another user) |
| `agent_groups_unexpected` | ST-02 | the agent's groups are not exactly its primary group plus `systemd-journal` (when it exists); carries the gids |
| `skill_account_invalid` | ST-03 | `domovoi-skill` is missing, is uid 0 or the agent's, has a login shell, or belongs to any group besides its own |
| `docker_group_member` | ST-04 | a Domovoi account is in the `docker` group (membership is root on a Docker host) |
| `unit_modified`, `unit_not_inspectable` | ST-07 | a differing property, any drop-in, or a pending reload; `systemctl show` failed or `FragmentPath` is empty (never a skip) |
| `unit_hardcoded_id`, `unit_exec_under_run` | ST-09 | a numeric uid or gid in a unit; an exec path under `/run`, `/tmp` or `/dev/shm` |
| `path_mode_wrong` | ST-10 | any path of the table off its mode, owner, group or type, or a write bit in the skills or packages tree; **reported, never repaired** |
| `socket_mode_wrong`, `socket_unreachable` | ST-12, ST-14 | the helper socket node is not `root:domovoi-agent 0660` (0666 fails); connect or `health` failed |
| `agent_reaches_helper_state`, `agent_reaches_docker` | ST-15, ST-16 | the agent could open the key, list the directory, open the lock or read `/proc/<pid>/environ` of a root process; the agent could connect to the Docker socket |
| `container_escape_surface`, `host_directory_created_by_mask` | ST-17, ST-18 | see 6.4 |
| `state_write_slow` | ST-20 | warn only |
| `runner_groups_extra`, `runner_privileges`, `runner_not_isolated{what}` | ST-05, ST-06, ST-13 evaluators | tested here, collected by 47b |

**Failure modes.**

- *Helper absent or erroring:* every helper capability is unavailable (`Unavailable(helper_missing|helper_error)` is the consumers' mapping of `HelperStatus::{Absent, Error}`); nothing falls back to running a privileged operation in the agent.
- *Start check fails:* the instance answers every request after the peer check with `StartRefused` and exits 0; the faults are also logged at error level; `helper_status` becomes `Error{reason: <token>}` within one probe interval. Nothing is changed on disk.
- *Peer wrong:* `Refused{peer_not_agent}`, logged at warn with the peer uid; no other field.
- *Slow or silent peer:* the read deadline (10 s on the `Clock`) ends the connection; the unit's `RuntimeMaxSec=60s` is the backstop.
- *A crash mid state write:* the old file survives (rename is atomic); a leftover temp file is removed under the lock on the next write; `DurabilityUnknown` is reported, never swallowed.
- *Selftest cannot inspect something* (a failing `systemctl`, an unreadable file): `fail` with `unit_not_inspectable` or `nd{reason}`, never `pass`.
- *`install.sh` finds a wrong mode or owner on an already-migrated host:* it stops before it enables the socket, prints each path with found and expected mode and owner, and says to run `domovoi helper repair`; it changes nothing.

## 11. Security requirements

1. **No write group exists.** The request enum has no write variant and no free-form string; a configured write group is ignored and reported. Test: `helper_request_has_no_write_or_free_form_variant`, `helper_ignores_write_groups_named_in_config`.
2. **Peer check.** Only the uid of `domovoi-agent`, resolved by name at connect time, is served; root and every other uid get `peer_not_agent`. Tests: `helper_rejects_wrong_peer_uid`, `helper_does_not_serve_root_peer`, `real_user_skill_cannot_connect_to_helper_socket` (P).
3. **No `unsafe`, no new dependency, no unset of the environment.** `adopt` uses std and `nix` features already in the workspace. Gates: the workspace lint, `adopt_has_no_unsafe_and_no_env_mutation` (source scan of the helper crate for `unsafe`, `remove_var`, `set_var`).
4. **A skill or package child never inherits `LISTEN_*` or the connection** (the existing spawn code; pinned): `spawned_skill_gets_no_listen_vars_and_no_connection`.
5. **Never repair at start; start is read-only.** `HostInspect` has no mutating method; the start-check module contains no mutating call. Gates: `helper_start_never_chmods_an_existing_directory`, `helper_repair_never_runs_at_start`, `helper_repair_is_not_reachable_from_start_agent_hub_or_chat`.
6. **Repair is owner-confirmed, local, bounded.** Root only, typed word at a terminal, no `--yes`, symlinks and wrong types refused, only listed paths, only mode and ownership, re-check afterwards. Tests: `helper_repair_requires_confirmation_and_prints_the_plan`, `helper_repair_refuses_symlinks_and_unexpected_types_and_reruns_the_checks`.
7. **The helper spawns nothing; only `domovoi-runner` spawns anywhere** (R3, R4). The self-test's `systemctl show` and the agent-run child go through the injected `ProcessExecutor`; the `--agent-checks` child is started with `run_as = domovoi-agent`, never a shell. Test: `selftest_spawns_only_through_the_executor` and the existing R3 scan.
8. **The helper never offers `run_as`, `sudo`, `systemd-run` or command execution.** There is no request that could carry one (item 1).
9. **The agent cannot reach helper state.** `agent_cannot_read_helper_state_directory_or_proc` (P): `EACCES` on the key path, the directory, the lock and `/proc/1/environ`, against the real helper binary and a fixture key.
10. **The accounts carry the control.** `domovoi-skill` has no supplementary group (checked by ST-03 through `getgrouplist` and, at runner start, ST-05 in 47b); neither account is in `docker` (ST-04); the installer verifies an existing account and never `usermod`s it. Tests: the `startup_check_*` evaluator tests, `docker_group_member_fails_selftest` (P), `installer_never_modifies_an_existing_account`.
11. **Tier honesty.** `nd` never satisfies a required check; a platform is never `Supported` while `SUPPORTED_PLATFORMS` is empty; the helper derives the write decision from its own record, not from the caller. Tests: `blocked_platform_refuses_skills_and_write_groups`, `untested_platform_allows_skills_with_notice_and_refuses_write_groups_without_opt_in`, `nd_never_satisfies_a_required_check`, `write_groups_are_refused_on_untested_platforms_without_owner_opt_in` (gate).
12. **No secret anywhere.** No key exists in 47a; `helper.toml` holds no secret; nothing logs or returns a credential, and no password is read or stored by any new code.
13. **Files and units.** Every shipped unit passes the unit rules; the helper socket is `0660 root:domovoi-agent`; the helper unit has no drop-in on a supported host (ST-07).
14. **Hub and agent interplay.** The hub cannot cause any of this: no API, command or message changes a mode, an account or `helper.toml`. Nothing under `crates/domovoi-hub/` changes.

## 12. Testing requirements

Fakes (existing unless marked): `FakeClock` (`advance`, `set`), `FakeProcessExecutor` (extend nothing; expectations per program and argv), `FixtureHostFs`, **new** `FakeHostInspect` (6.3). Tests never sleep: deadlines run on the fake clock; socket tests use `UnixStream::pair()` and tempdirs; owner checks in tempdir tests take an injected `StateLayout`/`(uid, gid)` so they pass as an ordinary user. Tier **U** runs in ordinary CI without root (`cargo nextest run -p domovoi-helper`, `-p domovoi-core`, `-p domovoi-config`, `-p domovoi-agent`, `-p domovoi`, `cargo test -p xtask`); tier **P** needs root, `useradd` and systemd as PID 1.

**Core** (`domovoi-core`, `identity`, `helper`, `isolation`): `agent_groups_exactly_primary_and_journal_pass`, `agent_groups_with_docker_or_root_fail`, `skill_account_with_any_extra_group_fails`, `skill_account_with_login_shell_or_agent_uid_fails`, `docker_membership_of_either_account_fails`, `startup_check_accepts_primary_group_only`, `startup_check_rejects_extra_group`, `startup_check_rejects_nonzero_capabilities`, `startup_check_requires_no_new_privs`, `path_table_matches_synthesis_5_7` (snapshot of `expected_paths` for the default dirs; review it), `evaluate_path_reports_found_and_expected_mode_owner_and_kind`, `symlink_at_any_listed_path_is_a_fault`, `skills_tree_with_group_write_bit_is_a_fault`, `helper_frame_rejects_length_above_cap_before_reading_body`, `helper_frame_round_trips_every_request_and_response`, `decode_request_maps_unknown_op_malformed_and_version`, `helper_request_has_no_write_or_free_form_variant`, `helper_group_write_set_is_exact`, `nonce_requires_64_lowercase_hex`, `check_ids_are_st01_to_st20`, `refusal_and_check_token_spellings_are_stable` (snapshot), `isolation_report_distinguishes_eacces_from_enoent`, `isolation_report_accepts_systemd_incoming_and_empty_run_and_rejects_any_other_entry`, `isolation_report_accepts_eacces_for_append_to_unowned_file_but_not_success`, `isolation_report_requires_ro_on_both_binds_and_erofs_for_create_rename_unlink`, `nd_never_satisfies_a_required_check`, `untested_platform_allows_skills_with_notice_and_refuses_write_groups_without_opt_in`, `untested_platform_with_opt_in_allows_write_groups`, `blocked_platform_refuses_skills_and_write_groups`, `blocked_platform_still_allows_helper_read_when_its_own_checks_pass`, `supported_requires_a_listed_platform`, `warn_never_changes_the_tier`, `boundary_not_in_effect_for_root_and_for_other_users`.

**Config:** `helper_config_default_is_read_groups_only`, `helper_config_rejects_unknown_key_group_and_duplicates`, `helper_config_ignores_write_groups_in_effective_set`, `helper_config_platform_opt_in_defaults_off`, `helper_config_accepts_the_documented_example` (the §8.3 owner example).

**Helper crate, adoption and serve** (`crates/domovoi-helper/tests/`): `refuses_listen_fds_greater_than_zero`, `accepts_listen_fds_unset_or_zero`, `refuses_malformed_listen_fds`, `adopts_a_connected_unix_socket_as_stdin` (descriptor-taking inner function over a pair), `refuses_a_non_socket_descriptor`, `adopted_stream_is_close_on_exec` (through `nix`, `cfg(unix)`), `adopt_has_no_unsafe_and_no_env_mutation` (gate), `serve_answers_health_and_groups`, `serve_answers_read_and_safety_attest_as_unavailable`, `serve_marks_a_disabled_group_group_disabled`, `helper_rejects_wrong_peer_uid`, `helper_does_not_serve_root_peer`, `serve_refuses_oversized_frame_without_reading_body`, `serve_refuses_unknown_operation_and_unknown_field`, `serve_closes_a_silent_peer_at_the_deadline_on_the_fake_clock`, `serve_answers_start_refused_with_every_fault_after_the_peer_check`, `helper_ignores_write_groups_named_in_config`.

**Helper crate, start checks, state, repair:** `helper_start_refuses_directory_mode_0755`, `helper_start_refuses_key_with_group_or_world_bits`, `helper_start_refuses_symlinked_key`, `helper_start_refuses_wrong_owner_and_wrong_type`, `helper_start_reports_every_offending_path_with_found_and_expected`, `helper_reports_permission_fault_not_corruption_for_unopenable_lock`, `helper_start_accepts_absent_key_and_state_in_this_milestone`, `helper_start_refuses_uninitialised_directory`, `helper_start_never_chmods_an_existing_directory` (gate: no mutating call in `start_check.rs`, no mutating method on `HostInspect`), `helper_init_creates_directory_0700_and_refuses_existing`, `helper_init_is_idempotent_on_a_correct_layout`, `state_lock_second_holder_gets_would_block_on_try`, `state_lock_refuses_a_symlink`, `write_atomic_locked_leaves_old_file_on_temp_sync_failure`, `write_atomic_locked_leaves_old_file_on_rename_failure`, `write_atomic_locked_reports_durability_unknown_on_dir_sync_failure`, `write_atomic_locked_removes_a_leftover_temp_file`, `write_atomic_locked_requires_the_guard` (compile-fail style: the signature takes `&LockGuard`; assert via the type in a doc test), `helper_repair_requires_confirmation_and_prints_the_plan`, `helper_repair_refuses_symlinks_and_unexpected_types_and_reruns_the_checks`, `helper_repair_refuses_without_a_terminal`, `helper_repair_never_runs_at_start` (gate), `helper_repair_is_not_reachable_from_start_agent_hub_or_chat` (gate: the repair module is referenced only from `cmd/helper.rs`).

**Helper crate, self-test:** `selftest_reports_every_check_with_its_id` (ST-01 to ST-20 appear, each with a reason string), `selftest_reads_unit_properties_through_an_instance_name_and_runtime_max_usec`, `selftest_parses_systemd_durations_6s_1min_infinity_and_composites`, `selftest_treats_a_failing_systemctl_show_as_unit_not_inspectable`, `selftest_flags_a_dropin_as_unit_modified`, `selftest_flags_exec_start_under_run`, `selftest_flags_hardcoded_numeric_ids_in_units`, `selftest_reports_group_path_and_socket_faults_with_found_and_expected` (ST-10, ST-12), `selftest_fails_on_world_connectable_socket`, `selftest_fails_when_agent_reaches_helper_state_or_docker`, `selftest_records_systemd_version_and_compares_nothing`, `selftest_warns_on_slow_state_writes_and_never_blocks`, `selftest_installer_obtains_agent_results_by_running_the_agent_checks_as_the_agent_account`, `selftest_spawns_only_through_the_executor`, `selftest_marks_runner_checks_nd_until_the_runner_reports`, `selftest_lists_skills_whose_run_as_is_not_domovoi_skill`, `selftest_record_writes_platform_json_atomically_under_the_lock`, `helper_groups_report_platform_record_missing_and_stale`, `probe_collect_fills_every_point_from_the_inspect_port`.

**Agent:** `helper_status_is_absent_without_a_socket`, `helper_status_is_ok_with_enabled_group_names`, `helper_status_is_error_with_token_on_refusal`, `helper_status_changes_within_one_probe_interval_on_the_fake_clock`, `helper_client_refuses_another_protocol_number`, `hand_over_chowns_only_what_differs_and_never_changes_modes`, `hand_over_refuses_a_symlink_anywhere_in_the_tree`, `enroll_as_root_hands_the_state_dir_to_the_agent_account` (only when the account exists; skipped otherwise), `agent_toml_is_written_0644`, `packages_boundary_is_in_effect_only_for_the_agent_account`, `packages_boundary_is_not_in_effect_for_a_nonroot_unexpected_user`.

**Runner:** `spawned_skill_gets_no_listen_vars_and_no_connection` (parent environment holds `LISTEN_FDS` and `LISTEN_PID`; the child prints its environment and `ls /proc/self/fd`; assert no `LISTEN_*` and no socket descriptor; `cfg(target_os = "linux")`).

**Binary** (`crates/domovoi/tests/helper_cli.rs`, `cfg(unix)`): `helper_serve_refuses_when_stdin_is_not_a_socket`, `helper_init_refuses_when_not_root`, `helper_repair_refuses_when_not_root_and_without_terminal`, `agent_selftest_prints_one_line_per_check_and_exits_nonzero_on_a_fail` (with the fake wiring behind a test seam), `top_level_selftest_is_unchanged`.

**Arch and gates** (`xtask/src/arch/*_tests.rs` and the engine and hub gates that already call the checker): `unit_parser_flags_runtime_max_with_oneshot` (UNIT-4), `unit_parser_flags_wide_socket_mode` (UNIT-5 with 0666), `unit_parser_flags_nonempty_supplementary_groups_and_docker` (UNIT-2 docker clause), `unit_parser_flags_dash_prefixed_mask_of_installer_path` (UNIT-7), `unit_parser_flags_agent_unit_without_user` (UNIT-8), `unit_parser_flags_writable_docker_socket_in_compose` (UNIT-8), `unit_parser_flags_exec_under_run` (UNIT-9), `unit_parser_flags_missing_or_changed_helper_directive` (UNIT-3), `installer_creates_every_created_path_before_enabling_sockets` (INSTALL-1 with a failing script), `helper_socket_and_request_are_named_only_in_allowed_files` (HELPER-1), `helper_state_paths_are_named_only_in_allowed_files` (HELPER-2), `skill_account_name_has_one_definition_and_no_numeric_ids_nearby` (IDENT-1, ID-2), `no_code_parses_systemd_instance_names` (ID-3), `every_selftest_check_has_a_test` (SELFTEST-1), `shipped_units_pass_every_unit_rule` (the real `deploy/` tree), `no_unit_execs_from_run`, `socket_units_are_0660_root_owned_group_domovoi_agent`, `agent_unit_runs_as_domovoi_agent`, `no_unit_or_compose_file_binds_docker_socket_read_write_into_the_agent`, `write_groups_are_refused_on_untested_platforms_without_owner_opt_in`, `rules_file_change_has_an_adr` (the existing `--base` check). Gates are never weakened.

**Installer (container, `deploy/test/installer.sh`, root, `ubuntu:24.04` and `debian:12`, no systemd):** the embedded `helper-socket`, `helper-service`, `hub` and `agent` units equal their files; with no `/run/systemd/system` the agent role takes the legacy path, creates no `domovoi-agent` or `domovoi-skill` account, prints the "privilege boundary not in effect" notice and keeps today's modes; the checksum and upgrade checks are unchanged.

**Privileged real-account tests (tier P).** `deploy/test/identities.sh` runs as root on a disposable VM with systemd as PID 1 (not in a container: masks and namespaces need mount privileges an unprivileged container lacks, and the hosted-runner image's systemd and cgroup state is to be confirmed on the chosen image). It builds nothing: it takes `DOMOVOI_BIN` (the release or debug binary) and the repository's `install.sh`, uses a fake release server like `installer.sh`, and **creates and removes only the real names** the installer creates (`domovoi-agent`, `domovoi-skill`, their groups and paths) after refusing to run when any of them already exists, plus `domovoi-probe-ci-*` accounts for the negative cases; a teardown `trap` deletes only what it recorded and the script fails if a `domovoi-probe-ci-*` object or one of the real names remains. Rows (each a named check):

- `fresh_install_creates_accounts_directories_and_units_with_the_table_modes`, `installer_never_modifies_an_existing_account` (a pre-made `domovoi-skill` in an extra group makes the installer fail with the account named), `installer_reports_wrong_modes_on_a_migrated_host_and_changes_nothing`, `installer_creates_mask_and_bind_paths_before_enabling_sockets` (INSTALL-1 at run time: the socket never starts before `/var/lib/domovoi-helper` exists);
- `helper_adopts_fd0_in_a_real_unit`, `helper_start_check_runs_in_a_real_unit` (a request over the real socket as `domovoi-agent` gets `health`; after `chmod 0755` of the helper directory it gets `StartRefused{helper_state_permissions}` and the directory is **still** 0755), `helper_restart_and_sigkill_keep_state_permissions`, `helper_unit_starts_under_the_shipped_directives` (no `226/NAMESPACE`, the instance exits 0, no `failed` unit left, `RemoveOnStop` removes the node);
- `real_user_skill_cannot_connect_to_helper_socket` (a probe account in no group gets `EACCES` at connect and the instance count is unchanged), `agent_cannot_read_helper_state_directory_or_proc` (with a fixture `attest.key` 0600 root), `skill_given_agent_group_is_stopped_by_helper_uid_check` (a probe account in `domovoi-agent` connects and gets `Refused{peer_not_agent}`), `docker_group_member_fails_selftest` (needs Docker; skipped with a notice otherwise), `selftest_fails_on_world_connectable_socket` (`chmod 0666` on the node), `selftest_detects_a_dropin_on_the_helper_unit` (`unit_modified`), `selftest_installer_passes_on_a_clean_host_and_marks_runner_checks_nd`;
- `legacy_upgrade_keeps_the_root_agent_and_prints_the_label`, `migrate_identity_converts_a_legacy_install` (state and packages owned by `domovoi-agent`, `agent.toml` 0644, unit replaced, saved copy present, agent active, ST-01 passes), `migrate_identity_refuses_when_a_dropin_sets_the_user`, `migrate_identity_refuses_when_the_hub_is_installed`, `migrate_identity_rolls_back_when_selftest_fails` (fault injection through a unit that cannot start), `rollback_identity_restores_the_legacy_unit_and_owners`.

How CI runs it: the script is gated by `DOMOVOI_PRIVILEGED_TESTS=1` (it exits 2 with a message without it); `.github/workflows/ci.yml` gets a classifier output `privileged` (changes under `install.sh`, `deploy/`, `crates/domovoi-helper/`) and a job `privileged` on `ubuntu-24.04` (a VM with `sudo`) that first prints `systemctl --version`, `stat -fc %T /sys/fs/cgroup` and `ps -p 1 -o comm=` and **fails with a clear message** when PID 1 is not systemd, then builds the binary and runs the script. The job runs on `workflow_dispatch`, on a nightly schedule and on pull requests that touch those paths, and is **not** added to the required `needs:` list at `ci.yml:500` until it has run green on three consecutive runs and the owner agrees; until then its result is recorded in the PR. Hosted Ubuntu 24.04 is the only image named here; Debian 13 and Ubuntu 26.04 are run by the owner on the research hosts and recorded as evidence, not as CI. Rust tests that need the real accounts (`crates/domovoi-helper/tests/privileged.rs`) are `#[ignore = "needs root, useradd and systemd as PID 1"]` and are run by the script with `cargo test -p domovoi-helper --test privileged -- --ignored`.

Run Linux-only tests in WSL as the repository rules describe, and say which ran where.

## 13. Acceptance criteria

1. Nothing under `crates/domovoi-proto/` or `crates/domovoi-hub/` changes; no golden frame changes; no migration.
2. A fresh agent install on a systemd host with no hub creates `domovoi-agent` (groups exactly its own and `systemd-journal`) and `domovoi-skill` (no group of any kind, never `docker`), every path of the 6.1 table with its mode, owner and group, runs `helper init`, installs the helper socket and template and the new agent unit, starts only the helper socket, runs `selftest --installer --record` and, on a failure, stops the socket and exits non-zero naming the check. It never starts the agent.
3. The installer never modifies an existing account, and in the fresh flow and on an already-migrated host never changes the mode or owner of an existing path (only the explicit migration converts the legacy paths listed in section 14): it reports each offending path with found and expected mode and owner and stops before enabling the socket.
4. A legacy root agent is left running and unchanged by a routine `install.sh --role agent` upgrade, which prints the "privilege boundary not in effect" notice and the exact migration command. `--migrate-identity` converts it as in section 14 (plan, typed confirmation at the terminal, saved unit and owners, stop, convert, helper, self-test, start) and rolls back on failure; `--rollback-identity` restores the saved state.
5. `domovoi helper serve` adopts the connection from fd 0 with std only, refuses a non-zero `LISTEN_FDS` and a non-socket, checks `SO_PEERCRED` against `domovoi-agent` (root refused), reads one frame of at most 65 536 bytes, answers `health`, `groups`, and `unavailable` for read-group and `safety.attest` requests, and exits 0. The request enum has no write variant and no free-form string.
6. A wrong or symlinked helper directory, lock, key, state or platform record, a wrong owner or type, an unopenable lock or an unparseable `helper.toml` makes the helper answer `StartRefused` with `helper_state_permissions` (or `helper_config_invalid`, `uninitialized`, `agent_account_missing`) and **every** offending path with found and expected values; nothing on disk changes; the start-check module has no mutating call.
7. `domovoi helper repair` is root-only, interactive-only, prints the plan, requires the typed word, refuses symlinks and wrong types, changes only listed paths' mode and ownership, re-checks and prints what is left; it is reachable from nowhere else.
8. `domovoi agent selftest` implements ST-01 to ST-20 with the tolerant forms of 6.4 (ST-07 through an instance name and parsed durations, ST-13 points 3 and 4, the eight-point evaluator), reports `nd` for the runner halves, reports `boundary` and the tier decision, lists skills whose `run_as` is not `domovoi-skill`, and exits non-zero on any `fail`. `--installer` obtains the agent-run results by running the agent checks as `domovoi-agent` through the injected executor.
9. `classify_platform` implements the tier table: `nd` or `fail` in a required check is Blocked; `SUPPORTED_PLATFORMS` is empty; Untested allows skills with the notice and refuses helper write groups unless `[platform] allow_write_groups_on_untested = true`; the helper refuses write groups when `platform.json` is missing or stale; every write group is unavailable in 47a regardless.
10. The agent reports `helper_status` from a real probe at start and every 60 s; `domovoi agent enroll` and `journal init|reset` run as root hand the state directory to `domovoi-agent`; `agent.toml` is written `0644`; the packages service reports the boundary from `BoundaryState`.
11. The unit files match 6.11 exactly; the arch rules of 6.12 exist with cards and failing-input tests, `cargo xtask arch-check` and `--base` pass with the ADR; `domovoi-helper` has the edges of the ADR and nothing else.
12. `deploy/test/identities.sh` has been run on at least one real host with systemd as PID 1 (host, distribution and systemd version recorded in the PR) and every row above passes, or the failing row is reported as a failure; **no platform is declared Supported**.
13. All tests named in section 12 exist and pass (tier U in CI; tier P as recorded); non-test files are at most 500 lines; no `unsafe`, no new `#[allow]`, no `unwrap`, `expect` or `panic!` in library code; no new workspace dependency.

## 14. Migration and compatibility

**Install cases (for `install.sh --role agent`).** The installer classifies the host before it creates anything:

| Case | Detected by | What a plain run does |
|---|---|---|
| No systemd | no `/run/systemd/system` or no `systemctl` | today's behaviour, unchanged; prints "privilege boundary not in effect: no systemd on this host" |
| Hub on the same host | user `domovoi`, `domovoi-hub.service` or `/etc/domovoi/hub.toml` exists | today's behaviour; prints "privilege boundary not in effect: the hub and the agent share /etc/domovoi (root:domovoi), which the three-identities layout cannot satisfy; run the agent on its own host to get the boundary" |
| Legacy | the agent unit has no `User=domovoi-agent`, or `state_dir` is root-owned | the binary upgrade only; nothing of the new layer is created; prints the label and `sudo sh install.sh --role agent --migrate-identity` |
| Fresh | neither the agent unit nor `state_dir` exists | the full three-identities install (steps A below) |
| Migrated | the unit has `User=domovoi-agent` | the binary upgrade; verifies every table path and reports faults (never repairs); `selftest --installer --record` |

Container installs (`deploy/compose`, `deploy/unraid`, the `docker/` images; the agent image runs as `nonroot`), hosts without systemd, and the single-process mode (`deploy/systemd/single-box.conf`, the agent runs as the hub's user) are **not** touched. Under the new definition they report `boundary: NotInEffect` (the agent is not the `domovoi-agent` account), which corrects today's `!runs_as_root` rule that would call a container agent running as `nonroot` "in effect" (section 3, difference 8). They get no unattended execution (RW-39 hard limit, `04` §7.3).

**Steps A: `install_identities` (fresh install and the core of the migration), in this order (INSTALL-1).**

1. Accounts. `domovoi-agent`: `useradd --system --user-group --no-create-home --home-dir /var/lib/domovoi-agent --shell <nologin>` with `-G systemd-journal` when that group exists. `domovoi-skill`: `useradd --system --user-group --no-create-home --home-dir /nonexistent --shell /usr/sbin/nologin domovoi-skill`. Numeric ids are never chosen. An existing account is **verified, not changed**: primary group, shell, group list (`id -Gn`) and `docker` membership (`getent group docker`) must be as in 6.1 or the installer stops and names the difference. `useradd` is required (BusyBox `adduser` hosts are not systemd hosts).
2. Directories, each created only when absent with the table's mode, owner and group, then **verified**: `/etc/domovoi`, the skills directory, `/var/lib/domovoi-agent`, `/var/lib/domovoi-packages`; `/etc/domovoi/helper.toml` (the 6.6 default, `install -m 0600`, only when absent; the header comment of `install.sh` is updated). The starter skills are copied as today and then normalised (directories 0755, files 0644, files that had an execute bit 0755, owner `root:root`, no group or other write bit). The list of created paths is the `CREATED_PATHS` variable between `# BEGIN created-set` and `# END created-set` (UNIT-7, INSTALL-1 read it).
3. `domovoi helper init`.
4. The three unit files (`domovoi-helper.socket`, `domovoi-helper@.service`, `domovoi-agent.service`) with `/usr/local/bin/domovoi` rewritten to the prefix; an existing differing unit is kept and the new text is written to `<prefix>/share/domovoi/` as today, and ST-07 reports a drop-in or a changed property.
5. `systemctl daemon-reload`, then `systemctl enable --now domovoi-helper.socket`. This is the only service the installer starts.
6. `domovoi agent selftest --installer --record`. A non-zero exit runs `systemctl disable --now domovoi-helper.socket`, prints the report and exits 1.
7. Next steps printed: enroll with `sudo` (the files are handed to `domovoi-agent` automatically), then `systemctl enable --now domovoi-agent`; the output says the host stays Blocked for skills until the skill runner is installed.

**`--migrate-identity` (root, local, interactive; the explicit owner action of RW-39's migration).** Differences from `05` §4a, decided here: the migration is **not** triggered by a routine rerun of the installer, because it removes root access from checks the owner may rely on; it needs the flag and a typed confirmation. Steps:

1. **Preconditions.** Layout is Legacy; systemd present; no hub on the host; no drop-in under `/etc/systemd/system/domovoi-agent.service.d/` sets `User=`, `Group=` or `SupplementaryGroups=` (an owner who followed the old unit's advice to run unprivileged via a drop-in is told which file to remove; the installer never deletes an owner drop-in, and a custom user that is not `domovoi-agent` stays "boundary not in effect"); `state_dir` and `packages_dir` contain no symlink and no entry owned by a uid other than the recorded owner of the top directory. Any failure stops with the reason before anything changes.
2. **Plan, printed in full:** accounts to create; each path whose owner, group or mode will change (found, expected); `agent.toml` 0640 to 0644; the skills tree normalisation; the unit diff (the owner's edits to the legacy unit are replaced and the old file is saved); and what stops working: checks that need root (Docker through the socket, SMART, the journal of every unit) report errors until the helper's read groups carry them (47d implements the Docker, SMART, kernel-log and listening-socket reads; until 47d is merged these checks report errors on a migrated host), and skills whose manifest names a `run_as` have no runner until 47b. Then ask the owner to type `migrate` on the terminal (`/dev/tty`; there is no `--yes`, and the migration refuses without a terminal).
3. **Record** under `/etc/domovoi/identity-migration/` (0700): the legacy unit, the owner, group and mode of `/etc/domovoi`, the skills directory, `agent.toml`, `state_dir`, `packages_dir`.
4. `systemctl stop domovoi-agent` and wait for inactive (the journal handles the stop like any restart: an execution in flight ends outcome-unknown through the existing recovery).
5. Steps A 1 to 6 with the conversion rules: `chgrp domovoi-agent /etc/domovoi` (mode 0750 already), skills directory 0750 to 0755 and tree normalisation, `chmod 0644 agent.toml`, `chown -R -h domovoi-agent:domovoi-agent` on `state_dir` and `packages_dir` (created when absent), the new agent unit replaces the legacy one (saved first).
6. `systemctl start domovoi-agent`; wait up to 30 s for `active` and for `agent selftest` to pass ST-01 and ST-02 as that account.
7. **Rollback**, automatic on a failure in steps 5 or 6 and available as `--rollback-identity` afterwards: stop the agent; restore the saved unit; restore the recorded owner, group and mode of every recorded path (`chown -R -h` for the two data directories back to the recorded owner); `systemctl disable --now domovoi-helper.socket`; `daemon-reload`; start the legacy agent. Accounts, the helper directory, `helper.toml` and the unit files of the helper stay (inert, additive, never deleted automatically) and are listed in the output.

**The labelled state.** Until the owner migrates, a root agent reports `runs_as_root = true`; the agent's `BoundaryState` is `NotInEffect{AgentIsRoot}`; packages report `privilege_boundary = not_in_effect`; hosts get no unattended execution; `agent selftest` prints the label and fails ST-01. Nothing about a legacy root agent's existing behaviour (skills, `run_as` switching, Docker, SMART) changes in 47a.

**Other compatibility.** Existing hubs and agents interoperate unchanged (section 9). Existing `agent.toml` files keep working; a `0640` `agent.toml` is normalised only by the migration (and ST-10 reports it on a migrated host). Skills: between the merge of 47a and of 47b a **fresh** agent install has no runner, so its skills are unavailable; this is acceptable on the integration branch because nothing is released before 47b and 47c, and it is the correct fail-closed result (`0030` Decision 6). The `domovoi-skills` spelling in manifests, tests and the snapshot is untouched until 47b.

## 15. Documentation requirements

- `docs/adr/0033-helper-crate-and-isolation-rules.md` (new, expected number, section 7 step 2): the crate and its edges (A2), the rule list with their evidence, the decision that the helper spawns nothing and that the migration is explicit, and the residual items the owner accepts (unproven list of ADRs 0030 and 0031). Update ADR 0030 (open question A2 becomes decided and points to the new ADR) and ADR 0031 (Decision 6 notes that the installer and `domovoi helper repair` exist; B2's list stays empty and the notice wording is the one of 6.9) with a revision note; statuses stay `proposed`.
- `ARCHITECTURE.md`: §4 row for `domovoi-helper` and the `domovoi` edge; §15 an "Identity and helper invariants" paragraph (the helper cannot be reached by the hub, the runner or a package; no write group; start never repairs) and the rule ids; §7.2 a sentence on the agent's helper probe.
- `docs/book/src/install/binary.md`: the agent install now creates two accounts and the helper; the `sudo` enroll hands files to `domovoi-agent`; the migration (`--migrate-identity`, what it changes, what loses root access, rollback); the hub-on-the-same-host limit; `--print-unit helper-socket|helper-service`.
- `docs/book/src/install/upgrading.md`: a routine rerun never migrates; the label and the command.
- `docs/book/src/security.md`: the three identities and the three layers, in plain words; the tier model and that **no platform is Supported yet**; what the helper can and cannot do in this release; the repair command.
- `docs/book/src/reference/cli.md`: `domovoi helper serve|init|repair`, `domovoi agent selftest` (flags, exit codes, the check list with tokens); say that `domovoi selftest` is the hub pipeline test.
- `docs/book/src/reference/agent.md`: the helper probe and `helper_status` values; the new `agent.toml` mode.
- `docs/book/src/reference/helper.md` (new, add to `SUMMARY.md`): `helper.toml` keys, the fresh-install default, the `[platform]` opt-in, the unit text and why each directive is there. (`cargo xtask docs-config` covers hub and agent only; this page is written by hand and checked by `helper_config_accepts_the_documented_example`.)
- `docs/book/src/guide/hardening.md` and `troubleshooting.md`: no drop-ins on the helper and agent units (ST-07 reports `unit_modified`); how to read `unit_modified`, `path_mode_wrong` and `helper_state_permissions`, and the repair command; why `systemctl edit` on these units blocks skills later.
- `deploy/README.md`: the new files and `deploy/test/identities.sh`; `TESTING.md`: how to run the privileged script and what it needs.
- `CHANGELOG.md` is generated by the release tooling; do not edit it by hand.
- Run `cargo xtask docs-links`. Commits and the PR must not mention prompts, the build plan or coding agents.

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

Task-specific: no new third-party dependency is added anywhere. The new crate `domovoi-helper` uses workspace
entries only (`nix` with the features the workspace already enables, `tokio`, `serde`, `serde_json`, `thiserror`,
`tracing`, `uuid`, `jiff`, `async-trait`); the root `Cargo.toml` gains only the crate's own path entry. Prompt 44 must
be merged; if the merged ADR texts, the unit text of synthesis 02 or a name used here differ, use the merged text and
report each difference. Run Linux-only tests in WSL (`wsl.exe -d Ubuntu --`, `CARGO_TARGET_DIR=$HOME/domovoi-target`)
and say which tests ran where. Run `install.sh --migrate-identity`, `deploy/test/identities.sh` and
`domovoi helper repair` only on a disposable virtual machine, never on a machine you care about: they create
accounts and change owners and modes.

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

Task-specific checks: crates edited are `domovoi-core`, `domovoi-config`, `domovoi-helper`, `domovoi-agent`,
`domovoi`, `domovoi-runner` (one test), `domovoi-testkit` and `xtask`, so run `cargo nextest run -E 'rdeps(domovoi-core)'`,
`-E 'rdeps(domovoi-config)'` and `-E 'rdeps(domovoi-testkit)'`, `cargo test -p xtask`, `cargo xtask arch-check` (and
`--base origin/overhaul-investigation`), `cargo xtask docs-links`, `shellcheck install.sh deploy/test/*.sh`,
`sh deploy/test/installer.sh` in a root container where Docker is available, `systemd-analyze verify` on the shipped
units, and `DOMOVOI_PRIVILEGED_TESTS=1 sh deploy/test/identities.sh` on a root-capable VM with systemd as PID 1.
Nothing under `web/`, `crates/domovoi-proto/` or `crates/domovoi-hub/` changes. Commit as
`Rostislav <raikov0411@gmail.com>`; the PR title has one scope, for example
`feat(helper): create the three identities and the root helper skeleton`.

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
