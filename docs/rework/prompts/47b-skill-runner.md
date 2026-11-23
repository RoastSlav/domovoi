# 47b — Skill runner: the `domovoi-skill` process, its units and the agent's switch to it

| | |
|---|---|
| Branch | `skill-runner` |
| Milestone | H1 (second of four parts: 47a, 47b, 47c, 47d) |
| Depends on (must be merged) | 47a (`helper-accounts-and-skeleton`: `domovoi_core::{identity, isolation, helper}`, the `HostInspect` port and `FakeHostInspect`, the `domovoi-helper` crate with `adopt`, `probe` and `UnixHostInspect`, `domovoi agent selftest`, the unit parser and rules, the installer migration; the 47a brief was read while writing this one, the names marked "47a" below are the names of that brief (reconciled in `README.md`, "Cross-brief names"); if the merged code differs, use the merged names and report each difference (step 1)), 20 (wire 1.2 `skill_runner_status`, `runs_as_root`), 41 (`packages-agent`, merged in this tree), 44 (ADRs 0030 to 0032, proposed) |
| Can run in parallel with | nothing. Series order: 47a, 47b, 47c, 47d, 45, 46 (`README.md`). 47b and 47c are order-flexible but never concurrent; 47b and 47d add no migration (47c adds the next one). 47d, 45 and 46 follow. |
| Produces (shared contracts) | `domovoi_core::skill_runner::*` (runner protocol frames and refusal codes); `domovoi_runner::serve` and `SandboxExecutor::run_cancellable`; `SkillCatalog::load_one`; `domovoi_agent::skill_runner::*` (client, route, status); `domovoi skill-runner serve`; hidden `domovoi agent dirs`; `deploy/systemd/domovoi-skill-runner.socket` and `domovoi-skill-runner@.service`; `xtask` rules `UNIT-1`, `UNIT-6`, `RUNNER-1`, `RUNNER-2` and the runner clauses of `UNIT-2`, `UNIT-3`, `UNIT-5`; `deploy/test/skill-runner.sh`; one ADR |
| Consumes | 47a: `SKILL_ACCOUNT` and `AGENT_ACCOUNT` (`domovoi_core::identity::accounts`), `evaluate_runner_groups` (ST-05), the ST-06 evaluator, `IsolationReport` and `evaluate_isolation` (ST-13), `domovoi_helper::adopt::adopt_stdin_connection`, `domovoi_helper::probe::collect`, `HostInspect` and `UnixHostInspect`, the selftest result types, `CREATED_PATHS` in `install.sh`, `xtask/src/arch/units.rs`, rules `HELPER-*`, `IDENT-1`, `ID-2`, `ID-3`, `UNIT-4`, `UNIT-7` to `UNIT-9`, `INSTALL-1`, the agent status task in `runtime.rs` (`probe_status`, `STATUS_PROBE_INTERVAL`). 41: `PackageName`, `PackageVersion`, the package layout. 22: `ImplHash::from_parts`. Wire 1.2 `skill_runner_status` (not changed here). |
| Launch gate | 47a merged **and** R-9 accepted by the owner for building, exactly as 47a's "Launch gate" defines it (the decisions of ADRs 0030 and 0031 are final for building, unproven items are residual, production-binary evidence is an exit criterion). (The index lists R-3, R-9 and R-10 for 47 as a whole; R-3 concerns the helper and R-10 the attestation, neither is needed here.) |
| Suggested model | medium for the runner, agent and unit work; the unit directive list, the cancel design and the fail-closed rules are fixed below, follow them exactly. Keep the final review for the main session. |

## 1. Objective

Make skills run as the unprivileged account `domovoi-skill`, in a sandbox built by systemd, through a socket-activated
process, and make the agent stop spawning skills itself whenever the privilege boundary is in effect. Concretely:

1. **The runner process.** `domovoi skill-runner serve --skills-dir <abs> --packages-dir <abs>` adopts the one
   `Accept=yes` connection systemd hands it on fd 0 (47a's std-only function, no new dependency), reads one framed request, validates
   it against the files it can see, runs the skill through `domovoi-runner` in its own process group, writes one framed
   reply and exits 0. It loads no configuration file.
2. **Cancellation.** While a skill runs, the runner watches the connection. EOF (or any byte or error) kills the skill's
   process group (SIGTERM, bounded wait, SIGKILL), collects the child and exits 0. `RuntimeMaxSec` on `Type=simple` is the
   backstop. No `failed` unit is left behind by a cancel, a refusal or a timeout.
3. **The units.** A socket unit and a service template under `deploy/systemd/` with the corrected directive set (parent
   masks, binds back in, `ProtectHome=yes`, no child mask under a masked parent, nothing executed from `/run`),
   installed by `install.sh` with the skills and packages paths following `agent.toml`.
4. **`run_as`.** The runner refuses any manifest `run_as` other than `domovoi-skill`; the agent refuses first; `domovoi
   agent selftest` lists the affected skills (D-31).
5. **The agent switch.** With the boundary in effect (agent euid not 0) skills run only through the runner, never in the
   agent process; `skill_runner_status` is probed at start, every 60 seconds and after a transport error; a runner that is
   not `ok` makes every skill unavailable and never falls back to a local run. A root agent keeps the labelled legacy path.
6. **Spelling.** The five code sites that say `domovoi-skills` become `domovoi-skill`, as the first commit of the branch.
7. **Checks.** The runner start checks ST-05 and ST-06, the isolation probe behind ST-13, the runner part of ST-07, and the
   unit-file rules and tests of synthesis 02 section 7, with the real-user tests marked as needing a Linux host with root.

It does **not** create accounts or directories, write the helper, change the wire, enable package skills, narrow the view
per package, ship a security profile, or touch attestation (section 5).

## 2. Architectural context

**Three identities (ADR 0030, RW-39, D-29).** The control agent runs as `domovoi-agent`, the skill runner as
`domovoi-skill` (no supplementary group of any kind), the helper as root. The runner is kept from agent state, helper
state, both sockets and every part of `/etc/domovoi` except `skills/` by the mount mask in its unit, and by file modes.
Mode and group of the socket (`root:domovoi-agent 0660`), the peer uid and the mask are three separate layers; none
replaces another.

**Why the sandbox is the unit.** The runner never builds a namespace itself (decision D-41: no user namespaces at launch,
Ubuntu 24.04 and later restrict them for unprivileged accounts). systemd builds the view from the unit, so the unit text
below is part of the security design and is tested as such. A directive in a file proves nothing; the effective view is the
answer, which is why the runner can report what it sees (the probe, section 6.6).

**Accept=yes costs, accepted by the owner (D-36).** systemd does not cancel an instance when the client disconnects, so the
runner does (section 6.5). A non-zero exit leaves a `failed` instance until `reset-failed`, so every defined outcome exits
0. The instance name is an opaque counter-and-pid string whose format differs between systemd builds: **nothing parses or
builds it** (rule `ID-3`, 47a). `%i` is never a package id.

**The runner cannot read `agent.toml`.** `/etc/domovoi` is masked and mode 0750. The request carries no path. The skills
and packages roots therefore reach the runner as `ExecStart=` arguments rendered by the installer from the same values the
binds use; `health` returns them so the agent can detect drift after an `agent.toml` edit.

**The runner's reply is data from the less trusted identity.** A skill runs as the same uid as the runner and could
signal or (with Yama off) trace it. The agent therefore bounds, parses strictly, re-redacts and caps every reply, and its
authority never depends on the runner telling the truth: a forged "success" for a process the skill itself controls
escalates nothing. The runner holds no secrets; redaction stays in the agent (`requests/skills.rs:55-59` already redacts).

**Fail closed, never fall back (ADR 0030 decision 6).** No runner, or a runner that is not `ok`, means every skill is
unavailable on an unprivileged agent. It never runs as `domovoi-agent`, because that would let package code reach the
helper and agent state.

**Where the legacy path remains.** An agent with euid 0 (an install not yet migrated, a container, a host without systemd)
keeps today's behaviour, labelled "privilege boundary not in effect": it spawns the skill itself and honours `run_as` by
switching user. Checks and read tools keep the local executor everywhere; only skill execution moves.

**One request per connection.** The agent sends one request frame and keeps its write side open until it has read the
reply. Closing it earlier is a cancel. There is no streaming: the reply is one buffered frame (RW-39 says "streamed"; the
wire to the hub is a single `skill_result` message and the existing executor already buffers, so streaming would add a
frame type nothing consumes).

## 3. Repository context

Verified in this checkout (branch `research-synthesis`, HEAD `b8021381`). Re-open each before editing and report
differences. Line numbers are of this tree.

**The runner crate (what exists).**

- `crates/domovoi-runner/src/exec/spawn.rs:46-62`: `Command::new(program)` with `.env_clear().envs(env)`,
  `.current_dir(spec.cwd or "/")`, **`.process_group(0)`**, stdin `Stdio::null()` (or a pipe when `spec.stdin` is set),
  piped stdout and stderr, `.kill_on_drop(true)`. So "own process group", "cleared environment" and "no inherited stdin"
  already exist; they are pinned by tests here, not built. `kill_on_drop` kills the leader pid only, not the group.
  `resolve_user` (`:19-37`) returns `None` when the target user is the current euid and refuses a switch unless root;
  the uid and gid switch is `:63-65`. The `#[allow(clippy::disallowed_methods)]` at `:41` is the existing ADR 0004 exception.
- `crates/domovoi-runner/src/exec/capture.rs:60-120`: `drive(child, spec)` reads capped stdout and stderr, enforces
  `spec.timeout` with `tokio::time::timeout`, sends SIGTERM to the group, waits `TERM_GRACE` (2 s, `exec/mod.rs:23`), then
  SIGKILL, and finally SIGKILLs the group so nothing outlives the run. It has **no cancel input** and uses `tokio::time`,
  not the `Clock` port (the runner crate has no clock; existing tests use short real timeouts, `tests/runner/exec.rs:70-95,
  210-212`).
- `crates/domovoi-core/src/ports/process.rs:74-77`: `ProcessExecutor::run(&self, spec) -> Result<ProcessOutput,
  ProcessError>`. No cancellation parameter. **This port does not change.**
- `crates/domovoi-runner/src/catalog/mod.rs:132-160` `SkillCatalog::run_skill(skill, args, executor)` checks the skill is
  the catalogue's own, re-checks the entry (`catalog/checks.rs:109-127`), builds the spec with `skill_run::build_spec`
  (`skill_run.rs:28-56`, which copies `manifest.run_as` into `spec.run_as`, `:53`) and runs it. `SkillCatalog::load(dir)`
  loads **every** folder (`checks.rs:34-60`); `load_skill(folder)` (`:70`) is private and does not check that the folder
  lies inside a root.
- `ImplHash::from_parts(manifest, entry)` exists (`crates/domovoi-core/src/action/plan.rs:143-155`); `LoadedSkill::
  manifest_hash()` and `entry_script_digest()` supply the parts (`catalog/mod.rs:37-60`). This is RW-39's `impl_hash`.
- Manifest timeout is limited to 1 second to 1 hour (`crates/domovoi-core/src/skill/validate.rs:134-136`), output cap to 1
  KiB to 1 MiB (`:139`).
- `crates/domovoi-runner/Cargo.toml`: tokio features are `process, io-util, time, fs, rt, macros`; `net` and `sync` are
  missing and are needed (a feature of an existing dependency, not a new dependency). `nix` (`signal process user fs`) is
  a unix dependency; `crates/domovoi-agent/Cargo.toml` and `crates/domovoi/Cargo.toml` already have what the client needs.

**The agent (what exists).**

- `crates/domovoi-agent/src/requests/skills.rs`: `run_skill` (`:62-142`) admits (catalogue, `allow_risk`, args) then
  recomputes the plan hash, then **refuses every non-read skill** (`:110-125`: "no journal-backed execution path") and
  runs read skills through `ctx.catalog.run_skill(skill, &validated, &*ctx.exec)` (`:128`). `run_read_skill` (`:144-190`)
  calls the same at `:175`. These two sites are the only skill execution calls. The refusal of mutating skills stays
  exactly as it is (non-goal).
- `crates/domovoi-agent/src/requests/mod.rs:73-100` `RequestCtx` (field `exec: Arc<dyn ProcessExecutor>` serves skills,
  tools and commands). It is built in `runtime.rs:192-212` and, as a literal, in `crates/domovoi-agent/tests/common/
  skills.rs:95`. `AgentDeps` literals: `production.rs:47`, `tests/agent/runtime.rs:20`, `tests/agent/dry_run.rs:24`,
  `crates/domovoi/tests/agent_hub.rs:192`. `requests/mod.rs` is 467 lines and `requests/catalog.rs` 473: do not grow them
  past 500; put new code in new files.
- Privilege boundary today: `runtime.rs:106-113` `runs_as_root()` (euid 0). `packages/mod.rs:263-270` derives
  `PrivilegeBoundary::{InEffect, NotInEffect}` from it. The hub's registry does the same: `crates/domovoi-hub/src/
  capability/registry.rs:429-441` (`runner_refusal`) returns no refusal for a root agent and `SkillRunnerMissing |
  Unsupported | Error` for an unprivileged agent whose runner is not ready. **The hub side is already done and is not
  changed.**
- `skill_runner_status` is hard-coded `Absent` in `link/handshake.rs:55` and `:127` (defaults); `runtime.rs:262-270`
  builds `HelloExtensions` with `..default()`. `AgentStatusSource::set(&self, AgentStatus)` (`handshake.rs:110`) exists "so a
  probe can change them". `SkillRunnerStatus { Absent, Unsupported{reason}, Error{reason}, Ok{version} }` is in
  `crates/domovoi-proto/src/messages/handshake.rs:224-243`.
- `crates/domovoi-agent/src/time.rs:22` `sleep(clock, by)` is how the agent waits on the `Clock` port.
- Package skills: `packages/list.rs:9` `EXECUTABLE_REASON = "skill_runner_not_integrated"`, pinned at
  `packages/tests/install.rs:475-476` and `crates/domovoi-agent/tests/gates.rs:266`. Package skills are not in
  `SkillCatalog`, `hello.skills` or the catalogue page (41). A package install already refuses any skill that sets
  `run_as` (`packages/install.rs:336-341`, `SkillRunAsForbidden`).
- `domovoi agent` subcommands: `crates/domovoi/src/cmd/agent.rs:20-31` (`Run`, `Enroll`, `Status`, `Journal`); the binary
  wires through `cmd/mod.rs:34-60`. The existing `domovoi selftest` (`cmd/selftest.rs`) is the hub pipeline test and is
  unrelated; `domovoi agent selftest` is introduced by 47a.

**Units and installer (what exists).**

- `deploy/systemd/` holds `domovoi-hub.service`, `domovoi-agent.service` and `single-box.conf`. `install.sh` embeds the unit
  text in shell functions (`hub_unit`, `agent_unit`, from line 38 and 91), renders `ExecStart` with
  `sed "s|/usr/local/bin/domovoi|$bin|"` (`:336`), and `deploy/test/installer.sh:48-52` asserts `install.sh --print-unit
  ROLE | cmp` equals the file in `deploy/systemd/`. The CI job `units` (`.github/workflows/ci.yml:593-605`) runs
  `systemd-analyze verify` on the two units with a stub binary. `make_dir` is `install.sh:285`; the agent directories are
  made at `:325-327` as `/etc/domovoi 0750 root:root`, `/etc/domovoi/skills 0750 root:root`, `/var/lib/domovoi-agent
  0700 root:root`. `prefix` defaults to `/usr/local` (`:133`).
- `AgentSection` defaults: `skills_dir` `/etc/domovoi/skills`, `packages_dir` `/var/lib/domovoi-packages`, `state_dir`
  `/var/lib/domovoi-agent` (`crates/domovoi-config/src/agent.rs:52-63`, defaults `:295-303`, `AgentSection::default_dirs`
  `:285`). `domovoi agent enroll` writes `skills_dir = "<path>"` (`crates/domovoi-agent/src/config_file.rs:32-39`).
- No `deploy/systemd` file, no `skill-runner` subcommand and no `SO_PEERCRED`, `LISTEN_FDS` or fd 0 handling exists today
  (47a adds the adoption function).

**The five spelling sites (D-43), exact lines.** `crates/domovoi-core/src/skill/manifest.rs:102` (doc example, compiled as
a doctest), `crates/domovoi-core/tests/manifests.rs:16`, `crates/domovoi-core/tests/snapshots/
manifests__skill_manifest_json_snapshot.snap:53`, `crates/domovoi-runner/tests/runner/skill_run.rs:16` and `:74`. No test
pins a manifest hash as hex (`manifests.rs:336-339` computes the expectation at run time), so there is no hidden sixth
site in code; `grep -rn domovoi-skills` over the tree finds only these five plus planning documents (below).

**Introduced by 47a (names as defined in the 47a brief; if the merged code differs, use the merged names and report each difference).**

- `domovoi_core::identity`: `SKILL_ACCOUNT = "domovoi-skill"` and `AGENT_ACCOUNT` (`accounts.rs`, the only non-test
  definition of the literals, rules `IDENT-1` and `ID-2`), `evaluate_runner_groups(groups, primary)` (ST-05 evaluator), the
  path table `expected_paths`.
- `domovoi_core::isolation`: `CheckId`, `CheckResult`, `Outcome`, `IsolationReport`, `evaluate_isolation(&IsolationReport)
  -> Vec<IsolationFault>` (the eight points with the tolerant forms) and the evaluator of ST-06 over the text of
  `/proc/self/status`; `domovoi_core::ports::HostInspect` (read-only; `euid`, `supplementary_groups`, `user`, `read_text`,
  ...) and testkit `FakeHostInspect`.
- `domovoi-helper` crate: `adopt::{check_listen_env, adopt_connection, adopt_stdin_connection, AdoptError}`,
  `probe::collect(&dyn HostInspect, &ProbeArgs { helper_pid }) -> IsolationReport`, `inspect::UnixHostInspect`. The runner
  crate cannot depend on it (adapters never depend on adapters, R1); the binary `domovoi`, which 47a gives the edge to
  `domovoi-helper`, calls it and injects the results (6.3).
- `domovoi agent selftest` with ST-01 to ST-20; the runner halves of ST-05, ST-06, ST-07, ST-13 and ST-14 are `nd` until
  this change. `xtask/src/arch/units.rs` (the unit parser) with `UNIT-4`, `UNIT-7`, `UNIT-8`, `UNIT-9`, the helper clause of
  `UNIT-3`, the docker clause of `UNIT-2`, `UNIT-5` for the helper socket, `INSTALL-1`, and `HELPER-1`, `HELPER-2`, `IDENT-1`,
  `ID-2`, `ID-3` (`ID-3` already covers `domovoi-runner`).
- The installer: `CREATED_PATHS` between `# BEGIN created-set` and `# END created-set`; the accounts; every directory with
  its mode (`/etc/domovoi` `root:domovoi-agent 0750`, `/etc/domovoi/skills` `root:root 0755`, `/var/lib/domovoi-agent`,
  `/var/lib/domovoi-packages` `domovoi-agent 0755`, **created even when `agent.toml` has no `[packages]`**); `agent.toml`
  mode 0644; `User=domovoi-agent` in the agent unit; the status task in `runtime.rs` (`probe_status`, 60 s on the `Clock`)
  that keeps `skill_runner_status` `Absent`; the `privileged` CI job and `deploy/test/identities.sh`.

47a ships no runner unit, runner request type or runner subcommand. If any name above does not exist, stop and report.
The `domovoi-helper@.service` unit (47a) uses the same `StandardInput=socket` shape; 47b copies none of its directives.

**Differences between this brief's sources and the repository (resolved here; report if the code disagrees).**

1. RW-39 cites `manifest.rs:124-126` for `run_as`; it is `:147-149` now (doc example `:102`). RW-39's `exec/spawn.rs:19-34,
   63-64` is `:19-37` and `:63-65`; `agent.rs:48,51,264` is `:52-63` and `:295-303`.
2. Synthesis 02 puts `ExecStart=/usr/bin/domovoi skill-runner`. The installer's prefix defaults to `/usr/local`, so the
   shipped file keeps the literal `/usr/local/bin/domovoi` that `install.sh` already rewrites with `sed` (the agent unit's
   convention). The requirement is only that the path is absolute and not under `/run`, `/tmp`, `/home` or `/root` (`UNIT-9`;
   `ProtectHome=yes` hides `/home`). The subcommand is `skill-runner serve` to match `helper serve` (04 section 4); RW-39 and
   the ADRs say `skill-runner` and are updated (section 15).
3. Synthesis 02 renders `@RUNTIME_MAX_SEC@`, `@SKILL_MEMORY_MAX@`, `@SKILL_TASKS_MAX@`. Placeholders would break
   `systemd-analyze verify` and the `--print-unit | cmp` test, so the files hold concrete default values. `MemoryMax=` and
   `TasksMax=` have measured mechanism but no measured value: they are **not shipped** and not required by `UNIT-3`.
4. Synthesis 02 5.4 item 6 says the adoption "awaits the owner's confirmation"; 00 section 5a and D-40 record it as
   confirmed.
5. RW-39 lists `env (allowlisted names only)` in the request. Nothing needs it: the runner derives `DOMOVOI_SKILL` and
   `DOMOVOI_PARAM_*` from the validated arguments exactly as `skill_run::build_spec` does today. The request has no `env`.
6. The runner's timers use `tokio::time` like `capture.rs`, not the `Clock` port (the runner crate has none); the agent's
   probe loop and deadlines use the `Clock` port.

## 4. Exact scope

Files to **add** (non-test files at most 500 lines; split rather than exceed):

| File | Purpose |
|---|---|
| `crates/domovoi-core/src/skill_runner/{mod,frame,request,reply}.rs` | pure protocol types, frame codec, refusal codes (6.1) |
| `crates/domovoi-runner/src/serve/{mod,connection,run,roots,peer,start_checks}.rs` | the runner side: one connection, validation order, cancel, start checks, probe hook (6.4 to 6.6) |
| `crates/domovoi-agent/src/skill_runner/{mod,client,route,state,status}.rs` | the agent side: socket client, the route that replaces direct skill spawning, status state, the runner status probe (6.7, 6.8) |
| `crates/domovoi/src/cmd/skill_runner.rs` | `skill-runner serve` (6.3) |
| `deploy/systemd/domovoi-skill-runner.socket`, `deploy/systemd/domovoi-skill-runner@.service` | the units (6.9) |
| `deploy/test/skill-runner.sh` | privileged harness, probe names only (section 12) |
| `xtask/src/arch/runner_rules.rs` and its tests; 47a's `units.rs` extended | `UNIT-1`, `UNIT-6`, `RUNNER-*` and the runner clauses (6.10) |
| `docs/adr/00NN-skill-runner-process.md` | required because `xtask/src/arch/rules.rs` changes; takes the next free number |
| tests: `crates/domovoi-core/src/skill_runner/tests.rs`, `crates/domovoi-runner/tests/runner/serve*.rs`, `crates/domovoi-agent/tests/agent/skill_runner*.rs` and `tests/common/runner.rs`, `crates/domovoi/tests/skill_runner_cli.rs` | section 12 |

Files to **change** (minimal edits): the five spelling sites (first commit); `crates/domovoi-core/src/lib.rs`;
`crates/domovoi-runner/{Cargo.toml,src/lib.rs,src/exec/mod.rs,src/exec/capture.rs,src/catalog/mod.rs,src/catalog/
checks.rs,src/error.rs}`; `crates/domovoi-agent/src/{lib.rs,runtime.rs,production.rs}`, `requests/{mod,skills}.rs`,
`packages/list.rs` (doc comment only), `tests/common/skills.rs`, `tests/gates.rs` (extend), the four `AgentDeps` literals;
`crates/domovoi/src/cmd/{mod,agent}.rs`; `install.sh`, `deploy/test/installer.sh`, `.github/workflows/ci.yml`;
`xtask/src/arch/{mod,rules,units}.rs`; `ARCHITECTURE.md`, `AGENTS.md`, `TESTING.md`, the book pages and planning texts of
section 15.

## 5. Explicit non-goals

- **No accounts, directories or modes, no helper, no evaluators.** 47a creates `domovoi-skill`, `domovoi-agent` and every
  directory and mode, and owns the ST-05, ST-06 and ST-13 evaluators, the probe collector and the fd 0 adoption; 47b only
  uses them and asserts the directories (the units fail the start if a bind source is absent, `ST-10` reports a wrong mode).
- No helper socket client, no `HELPER-*` rules, no repair command (47a). **No attestation**, key lifecycle
  or `expected_safety` (47c).
- **No wire change.** Nothing under `crates/domovoi-proto/`; protocol 1.2; no golden frame changes. In particular no
  `run_as` field in `hello.skills`, no new `SkillFailureCode`, no new `UnavailableReason` (see 6.8 and "ambiguities").
- **No journal-backed skill execution.** Non-read skills stay refused by `run_skill` as today; the runner executes what the
  agent admits.
- **No package-skill enabling.** The agent sends only the `skills_dir` source. Package skills stay out of the catalogue,
  `hello.skills` and `SkillCatalog`; `EXECUTABLE_REASON` and its two pinned tests are not changed (only its doc comment).
  The runner does implement the `package` source (the request contract is fixed by RW-39 and the unit already binds the
  packages tree), tested at runner level; nothing sends it yet.
- **No per-package narrowing** (D-42, ADR 0031 B4): the view is the whole skills tree and the whole packages tree, read-only.
- **No hardening candidates** (`ProtectKernel*`, `RestrictNamespaces`, `RestrictAddressFamilies`, `SystemCallArchitectures`,
  `UMask`, `LockPersonality`, `RestrictRealtime`, `ProtectClock`, `MemoryMax`, `TasksMax`): each needs a measurement under
  the fixture skills first. A follow-up change adds each one it measures.
- **No AppArmor or SELinux profile, no user namespaces, no Landlock** (D-41). **No `Accept=no`, no fd 3 fallback, no
  `listenfd`** (D-40). No `unsafe`, no new dependency, no environment unsetting (edition 2024 makes `remove_var` unsafe;
  the runner spawns with `env_clear()`).
- **No tier computation, untested-platform notice or acknowledgement** (D-39, wording open): 47a's selftest owns the tier
  report; 47b supplies its inputs (ST-05, ST-06, ST-13, ST-07 runner rows).
- **No container or non-systemd support**: those hosts keep the legacy agent, labelled (ADR 0031 decision 8).
- **No change to checks, read tools or `run_tool`**: they keep the local executor.
- **No rewrite of the starter skills** under `examples/skills/`. Those that need root or the `docker` group
  (`restart_container`, `restart_unit`, `vacuum_journal`, the journal readers) will not work as `domovoi-skill`; this is a
  documented consequence (section 14), not fixed here.
- Do not weaken any gate test or arch rule; do not touch merged migrations (there is no migration here).

## 6. Required interfaces and contracts

Marked **fixed** (must match) or **convention** (follow repository style).

### 6.1 Protocol types (`domovoi_core::skill_runner`, new, pure, **fixed** semantics, **convention** names)

```rust
/// Version of the agent-to-runner protocol. Both sides refuse another value.
pub const RUNNER_PROTOCOL: u32 = 1;
/// Path of the runner socket; the only definition of the literal (rule RUNNER-1).
pub const RUNNER_SOCKET_PATH: &str = "/run/domovoi-skill-runner.sock";
/// Largest frame either side accepts: two streams of 1 MiB, each byte JSON-escaped to six, plus overhead.
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
/// Largest request frame (requests are small).
pub const MAX_REQUEST_BYTES: usize = 64 * 1024;

/// Frame = 4-byte big-endian payload length (1..=limit) then that many bytes of UTF-8 JSON.
pub fn encode_frame<T: serde::Serialize>(value: &T, limit: usize) -> Result<Vec<u8>, FrameError>;
pub fn frame_len(header: [u8; 4], limit: usize) -> Result<usize, FrameError>;
pub fn decode_payload<T: serde::de::DeserializeOwned>(payload: &[u8]) -> Result<T, FrameError>;
pub enum FrameError { Empty, TooLarge { len: usize, limit: usize }, Malformed(String) }   // thiserror

/// One request per connection. Unknown fields are refused (`deny_unknown_fields` on every struct).
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunnerRequest { Health(Header), Run(RunRequest), Probe(ProbeRequest) }
pub struct Header { pub protocol: u32 }
pub struct RunRequest {
    pub protocol: u32,
    pub execution_id: ExecutionId,
    pub source: SkillSource,
    pub skill: SkillName,
    pub impl_hash: ImplHash,                      // the agent's ImplHash::from_parts(manifest_hash, entry_script_digest)
    pub args: BTreeMap<String, String>,           // as validated by the agent; the runner validates again
    pub timeout_secs: u32,                        // a cap: the effective limit is min(this, manifest timeout)
    pub max_output_bytes: u32,                    // a cap, likewise
}
pub enum SkillSource { SkillsDir, Package { name: PackageName, version: PackageVersion } }
pub struct ProbeRequest { pub protocol: u32, pub helper_pid: Option<u32> }   // pid supplied by a root caller, for point 6

/// Exactly one reply per connection, or none after a cancel.
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunnerReply { Health(HealthReply), Run(RunReply), Probe(IsolationReport), Refused(Refusal) }
pub struct HealthReply { pub protocol: u32, pub version: String, pub skills_root: String, pub packages_root: String }
pub struct RunReply {
    pub exit_code: Option<i32>, pub signal: Option<i32>, pub stdout: String, pub stderr: String,
    pub truncated: bool, pub timed_out: bool, pub duration_ms: u64, pub argv_hash: String,   // 64 lowercase hex
}
pub struct Refusal { pub code: RefusalCode, pub detail: String }       // detail: one sentence, never an argument value
pub enum RefusalCode { ... }                                           // table in section 10; as_str() snake_case
```

No path, no environment and no user name is carried by a `RunRequest`. `RefusalCode::as_str` values are stable tokens.
`PackageName`, `PackageVersion` (`crates/domovoi-core/src/package/names.rs`) and `SkillName` have grammars that exclude
separators, so a source can never traverse.

### 6.2 `SkillCatalog::load_one` and the cancel seam (`domovoi-runner`, **fixed** behaviour)

```rust
impl SkillCatalog {
    /// Loads exactly `<root>/<name>` into a catalogue of one. Canonicalises the folder and refuses one that
    /// resolves outside the canonical `root` (a symlinked folder); every other check of `load` applies.
    pub fn load_one(root: &Path, name: &SkillName) -> Result<(Self, LoadedSkill), CatalogIssue>;
}

pub enum RunEnd { Finished(ProcessOutput), Cancelled }
impl SandboxExecutor {
    /// Like `ProcessExecutor::run`, but when `cancel` completes first the process group gets SIGTERM, then
    /// SIGKILL after `TERM_GRACE`, the child is collected, the pipes are drained as today, and `Cancelled` is returned.
    pub async fn run_cancellable<C>(&self, spec: ProcessSpec, cancel: C) -> Result<RunEnd, ProcessError>
    where C: std::future::Future<Output = ()> + Send;
}
```

`capture::drive` gains a cancel future (the existing `run` passes `std::future::pending()`); the `select!` is between the
existing timeout wait and `cancel`. `ProcessExecutor::run` and `ProcessSpec` are unchanged. The group is `child pid` (the
spawn code's `process_group(0)`), so the runner never signals its own group. `SkillCatalog::run_skill` keeps its
signature; the runner calls a sibling that takes the cancel future (`run_skill_cancellable`, same checks, same
`build_spec`).

### 6.3 The subcommand (**fixed** shape)

`domovoi skill-runner serve --skills-dir <abs> --packages-dir <abs>`: a top-level `SkillRunner(SkillRunnerCommand)` with one
variant `Serve`, like `helper serve` in 47a. It loads **no config file** and reads no environment except through the
adoption function. Order: refuse a relative or missing root (usage error, exit 2, message on stderr, which is a loud
`failed` instance by intent); adopt fd 0 through `domovoi_helper::adopt::adopt_stdin_connection` (47a: refuses non-zero
`LISTEN_FDS`, refuses a non-socket); build a `ServeContext` and call `domovoi_runner::serve::serve_connection(stream, ctx)`;
exit 0. The module lives in `crates/domovoi/src/cmd/skill_runner.rs`, the composition root. Because adapters never depend on
adapters, the runner library receives everything it needs from the binary through ports: an already-adopted `tokio::net::
UnixStream`, an `Arc<dyn HostInspect>` (47a's `UnixHostInspect`), and a `ProbeCollector` (a trait defined in `domovoi-runner`,
`fn collect(&self, helper_pid: Option<u32>) -> IsolationReport`, implemented in the binary over `domovoi_helper::probe::
collect`). Tests of the runner library therefore need neither fd 0 nor the helper crate.

Hidden helper for the installer: `domovoi agent dirs [--config PATH]` prints exactly two lines, `skills_dir=<path>` and
`packages_dir=<path>`, from the validated `agent.toml` (defaults when the file is absent, `AgentSection::default_dirs`).
It exits non-zero with a plain sentence when a path is not usable in a unit: not absolute, not normalised, contains a
space, `%`, `|`, `&`, `\`, a quote or a control character, lies under `/run`, `/tmp`, `/home`, `/root`, `/proc`, `/sys` or
`/dev`, equals or contains `/etc/domovoi` (a path strictly beneath it is fine), or contains `/var/lib/domovoi-agent` or
`/var/lib/domovoi-helper`.

### 6.4 Runner behaviour per connection (**fixed**, in this order; each stop writes a `Refused` frame then exits 0)

0. At process start: ST-05 and ST-06 (6.5). If either failed, **every** request, including `health`, is answered
   `Refused(runner_groups_extra | runner_privileges)` and the process exits 0.
1. Read one frame of at most `MAX_REQUEST_BYTES` within 10 seconds, else `bad_request`. Unknown field, bad JSON or a
   frame longer than declared is `bad_request`.
2. `protocol == RUNNER_PROTOCOL`, else `protocol_mismatch`.
3. Peer uid from `SO_PEERCRED` (tokio `peer_cred`). The uid of `AGENT_ACCOUNT` is resolved by name at this moment through
   `HostInspect::user` (47a's way; never cached, never hard-coded). `Run` is allowed only to that uid; `Health` and `Probe`
   are allowed to it and to uid 0 (root diagnostics: `selftest --installer`); anything else is `peer_not_allowed`. The
   decision is a pure function over `(peer, request kind, agent_uid)` tested without sockets. This is one layer; the mask
   and the socket mode are the others. (47a's helper accepts the agent only; the runner also accepts root for the two
   read-only requests because ST-13 is driven as root.)
4. `Health` answers `HealthReply` with the two roots it was started with. `Probe` answers the observations (6.6).
5. `Run`: resolve the root (`skills_root`, or `packages_root/<name>/<version>/skills`); `SkillCatalog::load_one` (a missing
   or outside folder is `skill_not_found`, a failed load check is `skill_invalid` with the catalogue message);
   recompute `ImplHash::from_parts(manifest_hash, entry_script_digest)` and compare, else `impl_hash_mismatch`; run
   `manifest.validate_args(&request.args)`, else `args_invalid`; check `run_as`: for a skills-directory source the manifest
   value must be absent or exactly `SKILL_ACCOUNT` (47a's constant, never a literal here: `IDENT-1`), for a package source it must be
   absent (41's `SkillRunAsForbidden` rule), else `run_as_unsupported`.
6. Build the spec with `skill_run::build_spec` and **set `spec.run_as = None`**: the runner never switches users. Set
   `timeout = min(request, manifest)` and `max_output_bytes = min(request, manifest)`. Use a pass-through redactor (no secrets
   here; the agent redacts). Run with `run_cancellable`. The cancel future is the connection watcher below.
7. Reply `Run(RunReply)` (a timeout is a normal reply with `timed_out = true`) and exit 0.

**Connection watcher.** After the request frame is read, the runner keeps the read half and awaits one more read. `Ok(0)`,
an error, or any byte means cancel. (Half-closing the write side after the request is therefore a cancel; the client
never does it.) A cancel produces no reply, kills the group as in 6.2, and exits 0. A failed write of the reply (peer
gone) is not an error: exit 0. Only a defect (a panic, an unexpected `Err` from internal code) exits non-zero.

### 6.5 Start checks (**fixed**)

At process start, through the injected `HostInspect` (47a; the real adapter in production, `FakeHostInspect` in tests):
ST-05 is `evaluate_runner_groups(&supplementary_groups(), primary gid of SKILL_ACCOUNT)`; a failure is
`runner_groups_extra` (the detail names the gids). ST-06 is 47a's evaluator over `read_text("/proc/self/status")` (`CapEff`
and `CapBnd` zero, `NoNewPrivs` 1); a failure is `runner_privileges`. Neither is repaired. 47b adds no evaluator and no
token; it adds the collection and the refusal behaviour of 6.4 step 0 and tests every branch with the fake.

### 6.6 Probe (**fixed**)

`Probe` makes the runner call its injected `ProbeCollector` (6.3) and return the `IsolationReport` unchanged. The
collection (`domovoi_helper::probe::collect`) and the eight-point evaluation (`evaluate_isolation`, with the tolerant
forms of synthesis 02 5.9.1, `EACCES` where `ENOENT` is expected counted as a failed mask) are 47a's; the verdict is
produced by the caller (`domovoi agent selftest`, ST-13). The runner never refuses to run skills because of a probe result
(a false verdict would block every skill, and the `/run/systemd` listing is unmeasured on two systemd builds); it refuses
only because of ST-05 and ST-06. 47b's work is the request plumbing, the root-or-agent peer rule, and the three-line
selftest wiring of 6.11.

### 6.7 Agent client and route (`domovoi_agent::skill_runner`, **fixed** behaviour, **convention** names)

```rust
pub struct RunnerClient { /* socket path, Arc<dyn Clock>, limits */ }
impl RunnerClient {
    pub fn new(socket: PathBuf, clock: Arc<dyn Clock>) -> Self;                      // default RUNNER_SOCKET_PATH
    pub async fn health(&self) -> Result<HealthReply, ClientError>;                  // connect 3 s, reply 3 s
    pub async fn run(&self, request: RunRequest) -> Result<RunReply, ClientError>;   // dropping the future closes the connection = cancel
    pub async fn probe(&self, helper_pid: Option<u32>) -> Result<IsolationReport, ClientError>;   // reply 10 s
}
pub enum ClientError { Absent, PermissionDenied, Connect(String), ClosedWithoutReply, TimedOut, Malformed(String), Refused(Refusal) }

/// How the agent executes a skill. The only code that decides local versus runner.
pub enum SkillRoute { Legacy(Arc<dyn ProcessExecutor>), Runner(Arc<RunnerRoute>) }
impl SkillRoute { pub async fn run(&self, skill: &LoadedSkill, catalog: &SkillCatalog, args: &ValidatedArgs)
    -> Result<SkillOutput, RouteError>; }
pub enum RouteError { RunnerUnavailable(String), RunAsUnsupported { run_as: String }, Refused { code: RefusalCode, detail: String },
                      TimedOut(SkillOutput), Exec(String) }
```

- `RunnerRoute::run`: refuse `run_as` other than absent or `domovoi-skill` **before** connecting; refuse when the shared
  runner state is not `Ok`; hold a permit from a semaphore of **8** concurrent runs (the unit allows 16 connections and
  health and probe need room; waiting is bounded by the request deadline); send `RunRequest` with `timeout_secs` and
  `max_output_bytes` from the manifest and `impl_hash = ImplHash::from_parts(...)`; read one frame; keep the write half open
  until the reply is read; map the reply to `SkillOutput` (convert `argv_hash` hex to bytes; cap both streams again at the
  manifest's `max_output_bytes`; the caller redacts as today). A transport failure sets the shared state to `Error` and wakes the probe loop.
- `SkillRoute::Legacy` runs `catalog.run_skill(skill, args, &*exec)` exactly as today, including `run_as` switching, and is the
  labelled path.
- `requests/skills.rs` changes in two places only: `:128` and `:175` call `ctx.skills.run(...)` instead of
  `ctx.catalog.run_skill(...)`; the mapping of `RouteError` to the existing `SkillFailureCode::Exec` message and of
  `TimedOut` to `SkillFailureCode::Timeout` keeps the wire unchanged. `RequestCtx` gains one field, `skills: SkillRoute`;
  `exec` stays for tools and commands.
- Selection: `production::skill_route(config, executor, clock, euid_is_root)` returns `Legacy` for euid 0 and `Runner`
  otherwise (a pure function, tested both ways). `AgentDeps` gains `skills: SkillRoute`; tests build `Legacy` explicitly.

### 6.8 Status probe and the `run_as` surface (**fixed**)

`status.rs` adds `probe_runner_status(client, expected_dirs, clock) -> SkillRunnerStatus` and the task in `runtime.rs` that
47a introduced for the helper (`probe_status`, at start and every `STATUS_PROBE_INTERVAL`, 60 s, on the injected `Clock`)
calls it next to the helper probe; it also runs once, bounded, before `HelloExtensions` is built so `hello` carries the real
status, and again at once after a transport error (the route wakes the task). The result is written into
`AgentStatusSource` keeping every other field (`current()`, change, `set()`); the link's change probe sends `agent_status`.
Mapping:

| Observation | `SkillRunnerStatus` |
|---|---|
| socket path missing | `Absent` |
| connect `EACCES` | `Error { reason: "runner_socket_permission" }` |
| health ok, protocol equal, both roots equal the configured `skills_dir` and `packages_dir` | `Ok { version }` |
| `Refused(runner_groups_extra)` or `Refused(runner_privileges)` | `Unsupported { reason: <token> }` |
| `Refused(protocol_mismatch)` | `Error { reason: "runner_protocol_mismatch" }` (an old agent next to a new runner after an upgrade until restart) |
| roots differ from the configuration | `Error { reason: "runner_roots_differ" }` |
| connect refused, closed without reply, timeout, malformed | `Error { reason: "runner_unreachable" }` |

Euid 0 (legacy route): no probe, status stays as today.

`run_as_unsupported` is **wire-neutral** in this change. `SkillSummary` has no `run_as`, `SkillFailureCode` and
`UnavailableReason` have no such value, and adding one needs a wire revision, a UI vocabulary entry and OpenAPI
regeneration. So: the agent refuses before connecting, the runner refuses again, the token `run_as_unsupported` leads the
failure message (`SkillFailureCode::Exec`, message `run_as_unsupported: <skill> names run_as <user>; only domovoi-skill is
supported on this host`), and `domovoi agent selftest` lists the affected skills as one row each with result `warn`, token
`run_as_unsupported`, and the skill and `run_as` named (`info` when none; a legacy root agent lists them as `info`
"honoured by the legacy path"). Carrying the reason to the hub is left to a later wire revision.

### 6.9 The units (**fixed**; reasons from synthesis 02 5.1 and 5.2)

`deploy/systemd/domovoi-skill-runner.socket`:

```ini
[Unit]
Description=Domovoi skill runner socket

[Socket]
ListenStream=/run/domovoi-skill-runner.sock
SocketUser=root
SocketGroup=domovoi-agent
SocketMode=0660
Accept=yes
MaxConnections=16
RemoveOnStop=yes

[Install]
WantedBy=sockets.target
```

`SocketMode=0660` is a hard maximum (a `0666` socket is protected by no directive). `RemoveOnStop=yes` removes the node a
stop leaves behind. `MaxConnections=16` loaded as written; behaviour at the limit is unmeasured, hence the agent's
semaphore of 8.

`deploy/systemd/domovoi-skill-runner@.service` (the default rendering; the installer rewrites the binary path and the two
paths):

```ini
[Unit]
Description=Domovoi skill runner (connection %i)
# No ConditionPathExists= for the bind sources: a condition would skip the unit silently. A plain bind of an absent
# path fails the start with 226/NAMESPACE, which is the wanted fail-closed behaviour.

[Service]
Type=simple
User=domovoi-skill
Group=domovoi-skill
StandardInput=socket
StandardOutput=journal
StandardError=journal
ExecStart=/usr/local/bin/domovoi skill-runner serve --skills-dir /etc/domovoi/skills --packages-dir /var/lib/domovoi-packages
Restart=no
NoNewPrivileges=yes
CapabilityBoundingSet=
AmbientCapabilities=
RestrictSUIDSGID=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
PrivateDevices=yes
ProtectProc=invisible
ProcSubset=pid
TemporaryFileSystem=/etc/domovoi:ro /var/lib:ro /run:ro
BindReadOnlyPaths=/etc/domovoi/skills /var/lib/domovoi-packages
RuntimeMaxSec=3720s
KillMode=control-group
TimeoutStopSec=5s
```

| Directive | Reason |
|---|---|
| `Type=simple` | `RuntimeMaxSec` is ignored with `Type=oneshot` |
| `User=`, `Group=`, **no `SupplementaryGroups=` line** | an empty value does not drop the account's own groups and would suggest it does; the account carries the control and ST-05 checks it |
| `StandardInput=socket` | the connection is fd 0; `LISTEN_FDS` is 0 |
| `StandardOutput=journal`, `StandardError=journal` | keeps stdout from being the connection; logs reach the journal under the `/run` mask (re-measure with the real runner, section 12) |
| `ExecStart=` absolute, not under `/run` or `/tmp` | `/run` may be mounted `noexec` (`203/EXEC`); `ProtectHome=yes` hides `/home` |
| `Restart=no` | one connection, one run |
| `NoNewPrivileges`, empty bounding and ambient sets, `RestrictSUIDSGID` | defence in depth; ST-06 reads the effect |
| `ProtectSystem=strict` | `/etc` and `/var/lib` read-only |
| `ProtectHome=yes` | required: `strict` alone leaves `/home`, `/root`, `/run/user/<uid>` writable |
| `PrivateTmp`, `PrivateDevices` | a private writable temp area; no device nodes |
| `ProtectProc=invisible`, `ProcSubset=pid` | other users' `/proc/<pid>` hidden (information hiding, not custody) |
| `TemporaryFileSystem=` for three parents | everything below becomes `ENOENT`: both sockets, agent and helper state, the key, `helper.toml`, `agent.toml`; also makes `skills/` reachable under `/etc/domovoi` 0750 |
| `BindReadOnlyPaths=` for two sources | plain binds: an absent source fails the start; never bind a `.sock`; systemd applies binds after the tmpfs masks |
| `RuntimeMaxSec=3720s` | backstop: 3600 s manifest maximum + `TERM_GRACE` 2 s + drain 1 s + 117 s margin; the runner's own limits come first |
| `KillMode=control-group`, `TimeoutStopSec=5s` | the cgroup kill removes `setsid` grandchildren; written out so a rule can assert it |

**Mask rule.** Mask a parent or a child, never a child under a masked parent (`226/NAMESPACE`, and `systemd-analyze verify`
stays silent). `TemporaryFileSystem=` on an absent path silently creates an empty host directory, so the installer creates
`/etc/domovoi` and `ST-18` checks it. A `-`-prefixed child fails open and is not used. Install no drop-ins. Directive order
in the file carries no meaning. A local drop-in that removes the mask line makes skills stop working (fail closed): `ST-07`
reports `unit_modified`, `ST-13` fails.

### 6.10 Installer and rules (**fixed**)

- **Rendering.** `install.sh` gets two embedded functions (`runner_socket_unit`, `runner_service_unit`) and `--print-unit`
  roles `runner-socket` and `runner-service`. Installing the service renders three values with `sed`: the binary path
  (existing `/usr/local/bin/domovoi` rewrite), and `skills_dir` and `packages_dir` replacing the default paths **everywhere**
  they occur (the `ExecStart=` arguments and `BindReadOnlyPaths=`), both from `"$bin" agent dirs`. A non-zero exit of
  `agent dirs` aborts the runner steps and keeps any existing unit. It refuses a `--prefix` under `/home`, `/root`, `/run`
  or `/tmp` for the runner. The existing "keep a unit that differs, write the new one beside it" policy applies.
- **Order** (extends 47a's, `INSTALL-1`): the created-set directories exist (including `packages_dir` on a host without
  `[packages]`), then the two units are installed, `daemon-reload`, `domovoi agent selftest --installer`, and only if it
  passes `systemctl enable --now domovoi-skill-runner.socket`; a failed check leaves the socket stopped and names the check.
  The agent starts last. On a host without `/run/systemd/system` nothing is installed.
- **The created-set** is 47a's `CREATED_PATHS` list between `# BEGIN created-set` and `# END created-set` in `install.sh`
  (47a's brief lists the default skills directory and `/var/lib/domovoi-packages`; if the merged list holds the defaults only, 47b makes the installer add the
  configured `skills_dir` and `packages_dir` to it at render time and the rule checks both the default and a custom
  rendering).
- **Rules** (47a's `xtask/src/arch/units.rs` parser reused; the new rules go in `xtask/src/arch/runner_rules.rs`, and
  `rules.rs`, already 597 lines, gets only the registration; the ADR covers the change):

| Rule | Checks |
|---|---|
| `UNIT-1` | the runner unit has no `InaccessiblePaths=` entry under a `TemporaryFileSystem=` parent |
| `UNIT-2` (runner clauses) | the runner unit has no non-empty `SupplementaryGroups=` and `Group=` is exactly the skill account |
| `UNIT-3` (runner) | the runner unit has each directive of 6.9 with its value (not `MemoryMax`, `TasksMax` or any hardening candidate) |
| `UNIT-5` (runner socket) | the runner socket is `SocketMode=0660`, `SocketUser=root`, `SocketGroup=domovoi-agent`, `ListenStream=/run/domovoi-skill-runner.sock` |
| `UNIT-6` | no `BindPaths=`/`BindReadOnlyPaths=` entry ends in `.sock`; no `/run` child is bound |
| `RUNNER-1` | the literal `domovoi-skill-runner.sock` and the type `RunnerRequest` appear only in core's `skill_runner` module, `domovoi-agent/src/skill_runner/`, `domovoi-runner/src/serve/`, `crates/domovoi/src/cmd/skill_runner.rs`, testkit, test paths and `deploy/` |
| `RUNNER-2` | `domovoi-runner/src/serve/` imports no helper client, no `domovoi_helper` item and no agent state type |

`UNIT-4`, `UNIT-7`, `UNIT-9`, `INSTALL-1`, `ID-2`, `ID-3`, `IDENT-1` and `HELPER-*` are 47a's and now also run over the new
files (47a's gate `shipped_units_pass_every_unit_rule` covers the real `deploy/` tree). The parser tolerates the `%i`
specifier and ignores unknown directives. None replaces the run-time checks; a file can say anything.

### 6.11 Selftest wiring (**fixed**, small)

47a's `domovoi agent selftest` reports the runner halves as `nd`. 47b makes them real, with no new check id:

- **ST-05, ST-06**: the runner answers `Refused(runner_groups_extra | runner_privileges)` to `Health`; the selftest reads
  it through `RunnerClient::health` and turns the token into the `Fail` result; otherwise `Pass`.
- **ST-13**: `RunnerClient::probe(helper_pid)` (as root, with the helper's pid from 47a's `health` answer when the helper
  runs), then `evaluate_isolation`; a connection that fails is `nd{runner_probe_unavailable}`, never `Pass`.
- **ST-07**: the runner socket and `domovoi-skill-runner@selftest.service` join the queried units with their shipped values
  (`Accept`, `SocketMode`, `Type=simple`, `User`, `RuntimeMaxUSec` = 3720 s). The `ExecStart` argument vector and
  `BindReadOnlyPaths` must also equal the rendering for the current `agent.toml` (`skills_dir`, `packages_dir`): a
  difference is `unit_modified` naming `bind_paths` (config edited after the unit was rendered).
- **ST-12, ST-14**: the runner socket node (`root:domovoi-agent 0660`, socket type) and a `health` connect.
- **A new listing row, id `SK-01`** (not an `ST` id; it never gates the platform tier): one row per skill whose manifest
  `run_as` is neither absent nor `SKILL_ACCOUNT` (6.8).

`SELFTEST-1` (47a) requires a `// covers:` test for each token touched here.

## 7. Ordered implementation steps

1. **Inspect and verify; report differences.** Open every file, symbol and line of section 3. Confirm 47a is merged and read
   its real names (the list under "Introduced by 47a"); confirm there is no runner unit, subcommand or request type; confirm
   `ProcessExecutor::run` and `capture::drive` are as described; list `docs/adr/` for the next number. Write every
   difference down before editing; stop if one changes the design (for example if 47a placed the fd 0 adoption somewhere the
   binary cannot reach, or if `HostInspect` lacks `read_text`).
2. **First commit of the branch, alone: the spelling edit (D-43).** Change `domovoi-skills` to `domovoi-skill` at exactly
   `crates/domovoi-core/src/skill/manifest.rs:102`, `crates/domovoi-core/tests/manifests.rs:16`, `crates/domovoi-runner/
   tests/runner/skill_run.rs:16` and `:74`, and `crates/domovoi-core/tests/snapshots/manifests__skill_manifest_json_
   snapshot.snap:53`. Run `cargo insta review` and accept only that one line; run `cargo test -p domovoi-core --doc` (the
   manifest example is a doctest) and the two crates' tests. Subject: `chore(core): spell the skill account domovoi-skill`.
   `grep -rn "domovoi-skills" crates/` must find nothing afterwards. No other change in this commit.
3. **ADR and rules data**: the ADR, the `runner_rules.rs` shell and its
   registration, `ARCHITECTURE.md` rows. `cargo xtask arch-check` passes.
4. **Core protocol** (6.1): `skill_runner` module, the codec, `RefusalCode` with `ALL`; unit and proptest tests.
5. **Runner crate**: tokio features `net`, `sync`; `SkillCatalog::load_one` and `run_skill_cancellable`;
   `capture::drive` cancel input and `SandboxExecutor::run_cancellable` (6.2); then `serve` (6.4 to 6.6): peer rule, start
   checks, roots, request validation, the watcher. Runner tests with `UnixStream::pair()` and real short-lived child
   processes as `tests/runner/exec.rs` does. `spawned_skill_gets_no_listen_vars_and_no_connection` is 47a's test (`crates/domovoi-runner/tests/runner/exec.rs`); do not add a copy. The serve-path counterpart is `serve_spawned_skill_gets_no_listen_vars_and_no_connection`.
6. **Binary**: `cmd/skill_runner.rs` (`skill-runner serve`), hidden `agent dirs`, the `ProbeCollector` and `HostInspect`
   injection, selftest wiring (6.11). CLI tests.
7. **Agent**: `skill_runner` module, `RequestCtx.skills`, the two call sites, `production::skill_route`, the status task,
   the `AgentDeps` literals, the gates. Agent tests with a fake runner (a task over `UnixListener` in a tempdir) and the fake
   clock.
8. **Units, installer, scripts, CI** (6.9, 6.10): the two unit files, `install.sh` and `--print-unit`, `deploy/test/
   installer.sh`, `deploy/test/skill-runner.sh`, the `units` CI job rendering and verifying the new units, the privileged job.
9. **Rules and gates**: `UNIT-1`, `UNIT-6`, `RUNNER-1`, `RUNNER-2`, the runner clauses; negative tests; `cargo xtask
   arch-check` and with `--base origin/main`.
10. **Docs** (section 15), then the scoped checks and the completion report (hosts and results of the privileged harness).

## 8. Database and persistence

None. No migration, no table, no change to the journal, the audit log or any state file. The runner holds no state and
writes nothing (its filesystem view is read-only except a private `/tmp`).

## 9. Protocol and compatibility

**Hub wire:** unchanged (1.2). `skill_runner_status` and `runs_as_root` are existing `hello` and `agent_status` fields; the
hub registry already derives availability from them (`registry.rs:429-441`). **Agent-to-runner protocol:** new, local to one
host, version `RUNNER_PROTOCOL = 1`, framing 4-byte big-endian length plus JSON, one request and at most one reply per
connection, frame limits `MAX_REQUEST_BYTES` 64 KiB and `MAX_FRAME_BYTES` 16 MiB (worst case two 1 MiB streams, each byte
escaped to six, plus overhead). `domovoi upgrade` replaces the binary: new runner instances are the new version while a
running old agent still speaks the old one until restarted, which the version check turns into `Error{runner_protocol_
mismatch}`, never a misparse. A breaking change to this protocol bumps `RUNNER_PROTOCOL`. Golden frames: `insta` snapshots of
one encoded frame per request and reply kind, so a field rename is a visible diff.

## 10. Error handling and recovery

**Refusal codes** (`RefusalCode::as_str`, one `Refused` frame, then exit 0; `detail` is one sentence and never an argument
value):

| Code | When |
|---|---|
| `bad_request` | frame empty, over `MAX_REQUEST_BYTES`, not JSON, unknown field, truncated, or not read within 10 s |
| `protocol_mismatch` | `protocol` differs from `RUNNER_PROTOCOL` |
| `peer_not_allowed` | peer uid is not allowed for this request kind (6.4 step 3) |
| `runner_groups_extra` | ST-05 failed at start (every request is answered with it) |
| `runner_privileges` | ST-06 failed at start (every request is answered with it) |
| `skill_not_found` | no folder `<root>/<name>`, or it resolves outside the root |
| `skill_invalid` | the manifest or entry failed a catalogue load check (the catalogue message is the detail) |
| `impl_hash_mismatch` | recomputed `ImplHash` differs from the request's; nothing is executed |
| `args_invalid` | the runner's own `validate_args` failed |
| `run_as_unsupported` | manifest `run_as` is not allowed for the source (6.4 step 5) |
| `entry_changed` | the entry re-check before spawn failed |
| `spawn_failed` | `ProcessError` from the executor (not found, not executable, shell refused, spawn) |

**Agent mapping** (`RouteError`, never a wire change): `RunnerUnavailable` and `Refused` become `SkillFailureCode::Exec` with
the message `skill_runner_unavailable: <reason>` or `<code>: <detail>`; a reply with `timed_out` becomes `Timeout`;
`RunAsUnsupported` is `Exec` with the message of 6.8. A mutating skill is still refused by `run_skill` before any of this.

| Situation | Behaviour |
|---|---|
| Agent restarts or the hub deadline passes mid-run | the client future is dropped, the connection closes, the runner kills the group and exits 0; the outcome is "process killed, result unknown" exactly as 04 section 15.2 describes |
| Client half-closes or sends a stray byte after the request | cancel |
| Runner receives a request and the peer is gone before the reply | exit 0 |
| Skill ignores SIGTERM | SIGKILL of the group after `TERM_GRACE` (2 s); a `setsid` grandchild dies with the cgroup when the instance stops |
| Skill runs to the request or manifest timeout | the group is killed as today; a normal reply with `timed_out = true` |
| `RuntimeMaxSec` reached | systemd stops the instance (backstop, after the runner's own limits) |
| Runner crashes (defect) | non-zero exit, a `failed` instance until `reset-failed`; the agent sees `ClosedWithoutReply` and sets `Error{runner_unreachable}` |
| `MaxConnections` reached | the extra connection is closed unanswered; the agent's semaphore of 8 keeps this from happening in normal use |
| `ExecStart` binary replaced by `domovoi upgrade` | new instances run the new binary; no running instance is touched |
| Bind source absent (`skills_dir` or `packages_dir` missing) | the start fails `226/NAMESPACE`, the agent sees `Error{runner_unreachable}`, `ST-10` names the path; never a silent run without the bind |
| `agent.toml` edited to other directories | `health` roots differ: `Error{runner_roots_differ}`; the installer re-renders |

## 11. Security requirements

1. **Skills never run as `domovoi-agent` or root** when the boundary is in effect (euid not 0). The agent has no local path for
   them: `SkillRoute::Legacy` is constructed only in `production::skill_route` for euid 0 and in tests; a gate pins it.
2. **The runner never switches users**: `spec.run_as = None` always; it refuses `run_as` other than `SKILL_ACCOUNT`.
3. **The request carries no path, environment or user.** The runner resolves `<root>/<name>` itself, checks the canonical
   folder lies inside the canonical root, recomputes `ImplHash` (closing the gap between the agent's check and execution),
   re-validates the arguments, re-checks the entry (existing `recheck_entry`).
4. **The runner never fails open on an unprovable identity**: ST-05 and ST-06 failures refuse every request; a missing mask
   makes the skills directory unreachable under `/etc/domovoi` 0750 (outage, not leak); the probe reports it.
5. **No new `unsafe`, dependency or environment unsetting.** `LISTEN_FDS` non-zero is refused by 47a's adoption function;
   the spawn code's `env_clear()` and null or piped stdin mean a skill inherits neither `LISTEN_*` nor the connection (47a's test
   `spawned_skill_gets_no_listen_vars_and_no_connection` and 47b's `serve_spawned_skill_gets_no_listen_vars_and_no_connection`; the original fd 0 stays open in the runner by design).
6. **The reply is untrusted data**: strict parse, size bounds, re-capped and re-redacted by the agent; a forged reply cannot
   widen authority (section 2). A refusal never contains an argument value.
7. **Peer layers stay separate**: socket mode `0660` root:domovoi-agent (a hard maximum), peer uid (6.4), mask. No rule or
   check treats one as a substitute for another.
8. **Units**: parent masks alone, never a child under a masked parent, no `-` prefix on a created-set path, no `.sock` bind,
   `ProtectHome=yes`, no `SupplementaryGroups=`, nothing executed from `/run` or `/tmp`, no drop-ins installed; the installer
   creates every directory a mask or bind names before the socket is enabled and never repairs a wrong mode.
9. **The agent stays unable to widen the runner**: it cannot pass a root, a user, an environment or a path; the roots come
   from the unit.
10. **Logs** name the skill, the code and the execution id, never argument values (as `requests/skills.rs` does).

## 12. Testing requirements

Fakes (existing unless marked): `FakeClock`, `FakeProcessExecutor`, 47a's `FakeHostInspect` (accounts, groups, per-path facts and
`Access`, `/proc/self/status` text), real short-lived child processes for runner tests (as `crates/domovoi-runner/tests/
runner/exec.rs` does; the one place real time is unavoidable), `UnixStream::pair()` and tempdir `UnixListener` for the
socket, **new** `tests/common/runner.rs` in the agent: a fake runner task that replies with scripted frames, closes, stalls
or records whether the connection was closed. Tests that need Linux are `#[cfg(unix)]`/`target_os = "linux"`; run them in
WSL (`wsl.exe -d Ubuntu --`) and say which ran where. Tier P (real users, real systemd) is marked below.

**Core** (`crates/domovoi-core/src/skill_runner/tests.rs`): `frame_round_trips_every_request_and_reply_kind`,
`frame_longer_than_limit_is_refused_before_reading_the_body`, `request_with_unknown_field_is_refused`,
`run_request_has_no_path_env_or_user_field` (builds one and asserts the serialized keys), `refusal_codes_are_unique_snake_case`,
`reply_frame_snapshot` and `request_frame_snapshot` (`insta`, reviewed), a proptest `decode_never_panics_on_arbitrary_bytes`.

**Runner library** (`crates/domovoi-runner/tests/runner/serve*.rs`; names from 02 section 7.1 where they exist):
`runner_puts_skill_in_its_own_process_group` (child pgid differs from the runner's), `runner_kills_skill_group_on_eof_and_exits_zero`
(closing the peer ends the skill and a grandchild in its group; the serve future returns `Ok`), `runner_cancels_on_half_close_and_on_stray_byte`,
`runner_does_not_signal_its_own_group`, `runner_enforces_request_timeout_before_runtime_max`, `request_caps_lower_but_never_raise_manifest_limits`,
`runner_refuses_run_as_other_user` (`run_as_unsupported`, exit 0, nothing executed), `runner_accepts_run_as_absent_or_skill_account`,
`runner_never_sets_spec_run_as`, `package_source_refuses_any_run_as`, `runner_refuses_impl_hash_mismatch_without_executing`,
`runner_refuses_invalid_args`, `runner_refuses_folder_symlinked_outside_root` and `skill_runner_refuses_path_outside_skill_roots`,
`runner_refuses_entry_changed_since_load`, `runner_refuses_bad_frames_oversize_unknown_field_and_slow_requests` (the slow one uses a
paused tokio clock), `runner_refuses_protocol_mismatch`, `peer_rule_allows_agent_for_run_and_root_only_for_health_and_probe`
(pure), `runner_start_check_accepts_primary_group_only`, `runner_start_check_rejects_extra_group`, `runner_start_check_rejects_nonzero_capabilities`,
`runner_start_check_requires_no_new_privs` (serve level; the evaluator-level `startup_check_*` tests are 47a's), `failed_start_check_refuses_every_request_including_health_and_exits_zero`,
`health_returns_the_roots_it_was_started_with`, `probe_returns_the_collector_report_unchanged`,
`serve_spawned_skill_gets_no_listen_vars_and_no_connection` (parent environment holds `LISTEN_FDS` and `LISTEN_PID`; the child lists
its environment and `/proc/self/fd`; no `LISTEN_*`, fd 0 is null or the pipe, no socket), `load_one_loads_exactly_one_skill`,
`run_cancellable_without_cancel_behaves_like_run` (the existing exec tests keep passing). Adoption (`runner_adopts_stdin_connection_
without_unsafe`, `refuses_listen_fds_greater_than_zero`, `refuses_a_non_socket_descriptor`) is 47a's; this change adds no copy.

**Agent** (`crates/domovoi-agent/tests/agent/skill_runner*.rs`): `route_sends_one_request_and_keeps_write_side_open_until_reply`,
`dropping_the_run_future_closes_the_connection`, `route_maps_reply_to_skill_output_and_recaps_streams`,
`route_maps_timed_out_reply_to_timeout_failure`, `route_refuses_run_as_before_connecting` (the fake runner sees no connection),
`route_refuses_when_status_is_not_ok_and_never_runs_locally`, `route_limits_concurrency_to_eight`,
`route_marks_error_and_wakes_probe_on_transport_failure`, `status_absent_when_socket_missing`, `status_error_on_permission_denied`,
`status_ok_requires_equal_roots`, `status_unsupported_on_start_check_refusals`, `status_error_on_protocol_mismatch`,
`status_probe_runs_at_start_every_interval_and_after_error` (fake clock), `hello_carries_the_real_runner_status`,
`production_route_is_legacy_only_for_euid_zero` (pure function, both values), `non_root_agent_never_spawns_skills_locally` (a
non-root `AgentDeps` with a recording executor and a stub runner: the executor sees no skill spawn), `root_agent_keeps_legacy_run_as_switch`,
`run_skill_and_read_skill_use_the_same_route`, `mutating_skill_is_still_refused_before_the_route`. Gate in `tests/gates.rs`
(extend, never weaken): `legacy_skill_route_is_built_only_for_root_agents` (source scan: `SkillRoute::Legacy(` appears only in
`production.rs` and `skill_runner/route.rs`), `requests_never_call_catalog_run_skill_directly`; the existing
`skill_runner_not_integrated` assertion stays.

**Binary** (`crates/domovoi/tests/skill_runner_cli.rs`): `skill_runner_serve_refuses_relative_roots_with_exit_two`,
`skill_runner_serve_refuses_a_tty_stdin` (via 47a's adoption error), `agent_dirs_prints_defaults_without_a_file`,
`agent_dirs_prints_configured_dirs`, `agent_dirs_refuses_unit_unsafe_paths` (each class in 6.3), and with
`systemd-socket-activate --inetd -a` where available (skipped with a printed reason otherwise): one health round trip and one
EOF cancel with exit 0.

**Selftest wiring**: `selftest_turns_runner_start_refusals_into_st05_and_st06_failures`, `selftest_st13_is_nd_when_the_probe_connection_fails`,
`selftest_st07_flags_runner_bind_paths_that_differ_from_agent_toml`, `selftest_lists_run_as_unsupported_skills_as_warn`,
`selftest_lists_run_as_as_info_on_a_root_agent`.

**Unit files and installer** (xtask tests and `deploy/test/installer.sh`; named after 02 section 7.1): `unit_parser_flags_
rw39_inaccessible_child_under_masked_parent` (the literal RW-39 list fails `UNIT-1`), `unit_parser_flags_nonempty_supplementary_
groups`, `unit_parser_flags_missing_protect_home` (`UNIT-3`), (`UNIT-4` is 47a's test `unit_parser_flags_runtime_max_with_oneshot`; the runner unit passes it through `shipped_units_pass_every_unit_rule`),
`unit_parser_flags_wide_runner_socket_mode` (`UNIT-5` with 0666), `unit_parser_flags_socket_bind_into_runner` (`UNIT-6`),
`unit_parser_flags_dash_prefixed_mask_of_created_path` and `unit_parser_flags_path_not_in_created_set` (`UNIT-7`, covering
`TemporaryFileSystem=` and the two binds), `unit_parser_flags_exec_start_under_run` (`UNIT-9`), `shipped_runner_units_pass_every_unit_rule`
(default values) and `rendered_runner_units_for_custom_dirs_pass_every_unit_rule`, `runner_rules_have_negative_tests`,
`runner_socket_literal_is_defined_once` (`RUNNER-1`), `runner_serve_imports_no_helper_or_agent_state` (`RUNNER-2`),
`installer_creates_runner_paths_before_enabling_the_socket` (`INSTALL-1`, parse of `install.sh`),
`installer_embedded_runner_units_match_deploy_systemd` (the `--print-unit | cmp` rows), `installer_renders_custom_skills_dir_
into_exec_start_and_bind` and `installer_refuses_a_prefix_under_home_for_the_runner`, `installer_keeps_an_edited_runner_unit`,
`installer_leaves_the_socket_stopped_when_selftest_fails`. The CI `units` job renders both new units with the stub binary,
creates the two accounts first, and runs `systemd-analyze verify` (which proves syntax only: it is silent about the
`226/NAMESPACE` combination).

**Tier P: needs a Linux host with root, systemd as PID 1 and cgroup v2** (`deploy/test/skill-runner.sh`, POSIX sh like
`installer.sh`, refuses without `DOMOVOI_PRIVILEGED_TESTS=1`, **probe names only**: it renders copies of the units with
`domovoi-probe-ci-*` accounts, unit names and socket path, so it is safe next to a live agent, and a teardown trap removes
what it recorded and fails the run if a `domovoi-probe-ci-*` object is left). Cases (from 02 section 7.1): `real_user_skill_
cannot_connect_to_helper_or_runner_socket`, `real_user_skill_view_hides_sockets_state_and_key_under_corrected_unit` (the
eight-point report passes through the real runner and `evaluate_isolation`), `corrected_units_start_isolate_cancel_time_out_
and_clean_up` (start, ST-05, ST-06, ST-13, EOF cancel exits 0 with no `failed` unit, a `RuntimeMaxSec` kill of a `setsid`
grandchild with a short test value, `RemoveOnStop=yes`), `runner_writes_frames_to_inherited_socket_and_logs_reach_journal_
under_run_mask` (re-measure with the real runner), `runner_starts_on_a_host_with_no_packages_section` (the packages directory
exists, no `[packages]`), `polluted_skill_account_fails_start_check_and_is_still_masked`, `skill_given_agent_group_is_
stopped_by_mask_not_by_group`, `skills_dir_is_unreachable_without_mask_when_etc_domovoi_is_0750`, `protect_home_yes_closes_
home_and_root_writes`, `selftest_detects_dropin_that_removes_the_mask`, `noexec_run_blocks_direct_exec_under_run_while_usr_
bin_units_start` (a host with `noexec` `/run`), and the **canaries** that document systemd behaviour and fail when it changes
so the ADR row is revisited, not the product: `rw39_unit_text_fails_with_226`, `plain_mask_on_absent_path_fails_closed`,
`dash_mask_on_absent_path_starts_and_skips`, `temporary_filesystem_on_absent_path_creates_host_directory`,
`runtime_max_sec_is_ignored_for_oneshot`, `closing_connection_does_not_end_instance_without_runner_cancel`,
`failed_instance_lingers_until_reset_failed`. **How CI runs it:** extend 47a's `privileged` job (defined in 47a section 12: `ubuntu-24.04`, not in the required `needs:` list until it has run green three times in a row and the owner agrees);
it checks first that PID 1 is systemd and cgroup v2 is mounted and prints those facts. Whether a hosted `ubuntu-24.04` image
qualifies is **not known** and must be confirmed by the first run; if it does not, the job moves to a disposable self-hosted VM
(owner decision, none is assumed to exist) and the completion report says so. The real-name pass (real accounts) is the
extended `deploy/test/installer.sh` on a throwaway machine: runner socket and unit installed and rendered, `health` through the
real unit, one starter read skill executed through the runner, EOF cancel leaves no failed unit, `selftest --installer`
shows ST-05, ST-06 and ST-13 as `Pass`. Record the hosts, systemd versions and results in the PR.

## 13. Acceptance criteria

1. The first commit changes exactly the five spelling sites and the reviewed snapshot line; `grep -rn domovoi-skills crates/`
   is empty.
2. Nothing under `crates/domovoi-proto/` or any migration changes; no golden hub frame changes.
3. `domovoi skill-runner serve` adopts fd 0 with 47a's function, loads no config, serves exactly one request, and exits 0
   after a result, a refusal, a timeout, a cancel and a vanished peer; a defect is the only non-zero exit besides a usage error.
4. On EOF, a half-close or a stray byte the skill's process group is TERMed, KILLed after `TERM_GRACE`, collected, and the
   runner exits 0; the runner never signals its own group; the skill is in its own group; no `LISTEN_*` or connection reaches it.
5. The request carries no path, environment or user; the runner recomputes `ImplHash`, re-validates arguments, rejects a
   symlinked or foreign folder, and never switches users.
6. A manifest `run_as` other than absent or `domovoi-skill` is refused by the agent before connecting and by the runner, with
   the token `run_as_unsupported` in the message; `domovoi agent selftest` lists each such skill; the wire is unchanged.
7. With euid not 0 the agent runs no skill in its own process, ever; a runner that is not `ok` makes the skill fail with
   `skill_runner_unavailable`, and the hello and `agent_status` carry the probed `skill_runner_status`; with euid 0 the
   labelled legacy path behaves as before, including `run_as` switching.
8. The status is probed at start, every 60 seconds on the `Clock` port and after a transport error, with the mapping of 6.8;
   configured roots that differ from the runner's give `runner_roots_differ`.
9. The two unit files equal 6.9 for their default rendering; `install.sh` embeds them, `--print-unit` prints them, the
   installer test compares them, and a custom `skills_dir`/`packages_dir` is rendered into both the arguments and the bind.
   `UNIT-1`, `UNIT-6`, `RUNNER-1`, `RUNNER-2` and the runner clauses pass on the shipped files and fail their negative tests;
   `UNIT-7` covers `TemporaryFileSystem=` and both binds against the created-set.
10. Sockets are enabled only after the created-set directories exist and `selftest --installer` passes; a failed check leaves
    the socket stopped and names the check.
11. ST-05, ST-06, ST-07 (runner rows including bind drift), ST-12, ST-13 and ST-14 (runner halves) report real results.
12. Tier P and the real-name installer pass were run on at least one real host and recorded, or the report says exactly which
    were not run and why; every named test exists and the Linux ones passed in WSL.
13. `cargo xtask arch-check` (and `--base`) pass with the ADR; non-test files are at most 500 lines; no new `#[allow]`,
    `unwrap`, `expect`, `panic!`, `unsafe` or dependency (tokio features only).

## 14. Migration and compatibility

An agent that still runs as root (an install not yet migrated by 47a, a container, a host without systemd) keeps today's
behaviour, labelled "privilege boundary not in effect"; nothing changes for it except that the status probe is skipped. An
agent migrated by 47a (euid not 0) loses local skill execution with this change: until the runner socket is enabled by the
installer, skills fail with `skill_runner_unavailable` and the hub already shows them unavailable. Existing hubs and agents
interoperate unchanged (no wire change); a hub built before this change needs nothing. **Behaviour change for skill authors,
documented not fixed:** a skill now runs as `domovoi-skill` with a read-only working directory (its own folder), `HOME` set to
a directory that does not exist, a private writable `/tmp`, no `docker` group and no root; the starter skills that need those
(`restart_container`, `restart_unit`, `vacuum_journal`, the journal readers) report errors until the helper has operations
for them. Existing `run_as` values other than `domovoi-skill` stop working on a migrated agent (D-31); the owner rewrites the
skill. Re-running `install.sh` re-renders the runner unit; an edited unit is kept and the new text is written beside it, and
`ST-07` reports the difference. Package skills remain non-executable (a later enabling change). Rolling back: stopping the
runner socket makes skills unavailable; reinstalling a pre-47 binary and the legacy root unit restores the old path.

## 15. Documentation requirements

- `docs/book/src/guide/skills.md` (`run_as` row, line 90): only `domovoi-skill` is accepted on a migrated agent; what a skill
  can and cannot reach; the read-only working directory and the private `/tmp`; which starter skills need the helper.
- `docs/book/src/troubleshooting.md` (line 49, "permission denied"): the runner account, `skill_runner_*` statuses and the
  `run_as_unsupported` message, `systemctl reset-failed 'domovoi-skill-runner@*'`.
- `docs/book/src/security.md`, `docs/book/src/reference/agent.md`, `docs/book/src/reference/cli.md` (`skill-runner serve`,
  `agent dirs` is hidden and not listed), `docs/book/src/install/binary.md` (the units table at line 90 gains the two files)
  and `docs/book/src/install/upgrading.md` (re-render, `ST-07` drift).
- `ARCHITECTURE.md`: §4 rows (`domovoi-runner` also hosts the runner side; `domovoi-agent` the client), §7.2 (skills only
  through the runner when the boundary is in effect), §12 (the request, the reply, the cancel), §15 (an invariant: no skill
  runs as the control agent under the boundary) and the new rules.
- `TESTING.md` and `AGENTS.md`: the privileged harness, its environment switch, the extended installer test, and the one-line
  mention of the runner in hard rule 2 (only `domovoi-runner` spawns; the runner serves skills through it).
- `docs/adr/0034-skill-runner-process.md` (expected number, after 47a's 0033; list `docs/adr/` and take the next free one if different; extends 0030 and 0031; status `proposed`): crate placement for the
  runner (server in `domovoi-runner`, client in `domovoi-agent`, types in `domovoi-core`, binary as composition root; no new
  internal edge), the protocol decisions (one buffered reply, no `env`, version field, half-close is cancel), the wire-neutral
  `run_as_unsupported`, and the new rules. Run `cargo xtask docs-links`.
- Planning texts, as follow-up edits recorded in the PR (they are local to the V2 branch): RW-39 (replace the unit text and the
  `Request` list, the `skill-runner serve` spelling, `manifest.rs` citation), `04` section 8.4, `99` D-31 (the wire-neutral
  realisation), the 47b row of the prompt index. The ADR text of 0031 decision 3 already carries the unit; correct its
  `ExecStart` to the installer-rendered form.
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

Task-specific: no new dependency is added anywhere (tokio gains the `net` and `sync` features in `domovoi-runner`, which is
a feature of an existing dependency, not a new crate). 47a must be merged; if its names differ from the ones used here, use
the merged names and report each difference. The first commit of the branch is the spelling edit alone. Run Linux-only tests
in WSL (`wsl.exe -d Ubuntu --`, `CARGO_TARGET_DIR=$HOME/domovoi-target`) and say which tests ran where; say which privileged
tests ran on which real host.

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

Task-specific checks: crates edited are `domovoi-core`, `domovoi-runner`, `domovoi-agent`, `domovoi`, `domovoi-testkit` (only
if a fake is added) and `xtask`, so run `cargo nextest run -E 'rdeps(domovoi-core)'` and `-E 'rdeps(domovoi-runner)'`,
`cargo xtask arch-check` (and `--base origin/main`), `cargo xtask docs-links`, `cargo insta review` for the one reviewed
snapshot line plus the new frame snapshots, `shellcheck install.sh deploy/test/*.sh`, and `deploy/test/installer.sh` and
`deploy/test/skill-runner.sh` on a throwaway root-capable machine. No `web/` change. Commit as `Rostislav
<raikov0411@gmail.com>`; the PR title has one scope, for example `feat(runner): run skills as domovoi-skill through a
socket-activated runner`.

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
