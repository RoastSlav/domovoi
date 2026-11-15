# 54 — R-3 and R-9 helper delivery and skill-runner isolation research

| | |
|---|---|
| Branch | `spike-helper-and-skill-runner-platforms` |
| Milestone / research item | R-3 (helper delivery and platform support) and R-9 (skill-runner isolation); validation gate for H1 and prompts 45–49 |
| Dependencies | none. Read `03 RW-39`, `03 RW-37` (key and state surface only), `04 §8.3–8.4`, `99` R-3/R-9, D-24, D-29, O-30 |
| Can run alongside other research | yes: 53 and 56 are independent; 55 is separate and security-sensitive |
| Produces | R-3 and R-9 evidence in one report: `docs/rework/investigations/J-r3-r9-helper-and-skill-runner.md` (next free letter if `J` is taken) |
| Launch gate | Ubuntu WSL2 available (`investigations/H-wsl2-research-environment.md`); no other gate |
| Suggested model | medium |

## 1. Objective

Answer two related questions with measured evidence:

- **R-3:** can the helper be delivered and operated consistently on the platforms Domovoi targets —
  systemd socket activation, peer-UID authentication, sandboxing, root-owned state, containers,
  non-systemd hosts such as Unraid, and Docker API access — and which platforms must be unsupported or
  degraded safely?
- **R-9:** does the proposed skill-runner unit actually isolate `domovoi-skill` from the helper socket,
  the agent's private state and `/etc/domovoi/*`, and can it refuse writes to package and skill sources?

This is research. The deliverable is a report plus disposable probe units and scripts, not production
code. Nothing here authorises implementing the helper or the skill runner, and nothing accepts R-3 or R-9.

## 2. What this prompt is not

- It is not prompt 47 (`three-identities-helper-and-skill-runner`). No helper, runner, installer or
  product unit is installed or written.
- It is not R-10 (attestation). Do not design, implement or test signed attestation, key lifecycle
  semantics or `04b` vectors; another prompt covers that. You may create a dummy 0600 key-shaped file to
  test *permissions only*.
- It does not decide O-30 (`run_as` other than `domovoi-skill`). Do not assume the recommended option.
- It does not accept R-3 or R-9. Acceptance is recorded by the owner in the ADRs and decision register.

## 3. Architecture in plain language

The accepted direction (D-24, D-29, RW-39, `04 §8.3–8.4`):

- The control agent runs as the unprivileged user `domovoi-agent`.
- Skills and packages run as a separate identity, `domovoi-skill`, started only by a skill runner. The
  agent never spawns a skill itself.
- Privileged host work goes only through one narrow, root-owned helper. The helper offers typed operation
  groups — `docker.read`, `docker.write` (restart by container id), `systemd.write` (restart by canonical
  unit id plus config hash), `nft`, `smart.read`, `kernel.read`, `net.read`, `safety.attest` — and never
  `run_as`, `sudo`, `systemd-run` or command execution.
- The helper socket is `/run/domovoi-helper.sock` (0660, group `domovoi-agent`) started by
  `domovoi-helper.socket`; the helper checks `SO_PEERCRED` as **one layer only**.
- The skill-runner socket is `/run/domovoi-skill-runner.sock` (0660, group `domovoi-agent`); one runner
  instance per execution as `domovoi-skill`, with `NoNewPrivileges`, an empty capability set, no
  supplementary groups, `ProtectSystem=strict`, read-only skill and package roots, masked
  `/etc/domovoi`, `/var/lib` and `/run` except the necessary binds, and `InaccessiblePaths` for
  `/var/lib/domovoi-helper`.
- The helper is installed and updated only by the owner's OS install path; capability packages can never
  install or update it. The agent never receives generic Docker socket write access.
- The hub sees helper state only through a signed attestation relayed by the agent. The helper does not
  verify approvals or policy; its allowlist bounds operations, and the agent and hub bound authority.
- Fresh install enables read groups plus `safety.attest` only. Write groups require owner configuration
  and the milestone's prerequisites.

R-3 and R-9 exist because none of this is built and several of its guarantees have not been measured:
socket activation and peer credentials, whether an `Accept=yes` unit works for a Unix socket, whether the
sandbox directives produce the effective filesystem view the design assumes, what happens on hosts
without systemd, in containers, or where the Docker socket is mounted.

## 4. Repository and environment context

Current state (verify before relying on it):

- **The agent runs as root.** `deploy/systemd/domovoi-agent.service` has no `User=` and its comments say
  root is required; `deploy/systemd/domovoi-hub.service` runs as `domovoi`. `install.sh` creates only the
  `domovoi` user and group; agent paths are `/etc/domovoi` (0750 root:root), `/etc/domovoi/skills`
  (0750 root:root) and `/var/lib/domovoi-agent` (0700 root:root).
- **Skills run in-process as the agent's uid.** `crates/domovoi-agent/src/requests/skills.rs` calls the
  catalogue through `ProcessExecutor`; `crates/domovoi-runner/src/exec/spawn.rs` switches uid/gid only
  when `spec.run_as` is set and the process is root. A non-root process with `run_as` fails cleanly
  (`run_as_other_user_without_root_fails_clearly` in `crates/domovoi-runner/tests/runner/exec.rs`).
- **No helper or skill runner exists.** There is no `helper` subcommand (`crates/domovoi/src/cmd/mod.rs`),
  no `SO_PEERCRED`, no `LISTEN_FDS`, no socket-activated unit, no `flock`, no helper state or key.
- **No config for the new layout.** `crates/domovoi-config/src/agent.rs` has `state_dir`, `skills_dir` and
  `host_root`; there is no `packages_dir`, `[unattended]`, `[mutation_limits]` or helper configuration.
- **`run_as` naming mismatch.** `crates/domovoi-core/src/skill/manifest.rs` documents `run_as` and its
  example uses `domovoi-skills` (plural) while D-29/RW-39 name `domovoi-skill`. Record this as a finding;
  do not change it.
- Existing systemd inspection is state-only: `crates/domovoi-checks/src/units/` runs
  `systemctl list-units --state=failed` and `systemctl show` for `ActiveState`, `SubState`, `NRestarts`
  and `Result`, through fixed templates in `crates/domovoi-checks/src/commands.rs` (`/usr/bin/systemctl`,
  15-second timeout, 2 MiB cap). Nothing reads unit files or sandbox directives.
- Environment: `docs/rework/investigations/H-wsl2-research-environment.md` records Ubuntu 26.04.1 LTS,
  kernel `6.18.58.1-microsoft-standard-WSL2`, systemd 259.5 as PID 1, nftables 1.1.6, NAT networking.
  `sudo` was usable for the recorded probes; `AGENTS.md` warns that `sudo` may prompt for a password.
  Establish which is true now before starting.
- The design text is `docs/rework/03-architecture-decisions.md` RW-39 and RW-37, and
  `docs/rework/04-target-architecture.md` §8.3–8.4.

Proposed outputs of this prompt, and nothing else:

- `docs/rework/investigations/J-r3-r9-helper-and-skill-runner.md` — the report.
- Disposable probe units and scripts, kept in the report or under
  `docs/rework/investigations/probes/`. Do not add product code, units under `deploy/`, or changes to
  `install.sh`.

## 5. Evidence discipline

Label every finding as exactly one of:

- **demonstrated on WSL2** — observed here, with command and output;
- **not demonstrated** — untested or inconclusive;
- **requiring conventional Linux validation** — depends on another distro, systemd version, container
  runtime, physical host or non-WSL2 kernel.

Rules:

- A successful probe is evidence, not acceptance. Never write "R-3 is resolved", "R-9 is resolved" or
  "the helper is supported on Linux".
- Record the environment for every run: `uname -r`, `systemctl --version`, `systemd-analyze --version`,
  user and uid, whether the unit was system or user scope.
- Distinguish "the unit file contains the directive" from "the process actually could not read the path".
  The second is the only one that answers R-9.
- Where a test substitutes a different identity or path for the real one, say so explicitly and record
  what the substitution cannot prove.
- Commands in §7 and §8 are reference sequences: verify their syntax and identifiers before running them,
  and record the command you actually ran.

## 6. Safety and isolation

Mandatory:

1. Use only disposable units named `domovoi-probe-*`, preferably in `/run/systemd/system` so they vanish on
   reboot. Never touch a real `domovoi-*` unit; if an installation exists, stop and report.
2. Remove every unit, socket, drop-in, temporary user/group, directory and socket you create, with a
   `trap` that runs on failure too. `systemctl daemon-reload` after cleanup.
3. Do not install packages (`docker`, `ufw`, `firewalld`, `unraid` tooling). Absence is a finding.
4. Do not modify an existing `/etc/domovoi`, `/var/lib/domovoi-agent` or `/var/lib/domovoi-helper`. If
   they exist, treat them as read-only and say so. If they do not exist, you may create 0700/0750 test
   fixtures at those exact paths **only** after checking absence, and must delete them afterwards.
5. Test users and groups: prefer the existing unprivileged WSL user as the `domovoi-skill` substitute.
   You may create a disposable group such as `domovoi-probe-agent` to test socket ownership; delete it
   afterwards. Do not create `domovoi-agent`/`domovoi-skill` system users on this machine.
6. Never enable a helper socket on a real path and leave it listening. Every socket is created for the
   test and removed.
7. Do not run anything as root that exposes a shell, sudoers change, or persistent service. No
   `systemd-run` units may survive the session.
8. If `sudo` requires a password you do not have, stop and report; do not attempt workarounds.

## 7. Experiment matrix — R-3 (helper delivery and platforms)

| ID | Question | Method | Record | What it does and does not establish |
|---|---|---|---|---|
| H1 | Is `sudo`, systemd and `systemd-analyze` usable for unit tests? | `id`, `sudo -n true`, `systemctl --version`, `systemd-analyze --version`; try a disposable unit | Which privileges are available; exact systemd version | The test environment only. |
| H2 | Does Unix-socket activation work with `Accept=no`? | `domovoi-probe.socket` (`ListenStream=/run/domovoi-probe.sock`, `SocketMode=0660`) + service that prints `LISTEN_FDS`/`LISTEN_FDNAMES` and the peer credentials; start, connect twice | Whether the service starts on first connect; fd count and names; one vs two instances | Activation and fd passing on this systemd. |
| H3 | Does `Accept=yes` work for **AF_UNIX**? | Same socket with `Accept=yes` and an `@.service` instance; `systemd-analyze verify`; start and connect twice | Exact verification output or runtime error; whether one instance per connection appears; `systemctl status` | Whether the design's `Accept=yes` Unix-socket assumption holds on systemd 259.5. If it does not, record the supported alternative (a listener that accepts and spawns, `Accept=no`, or another mechanism) without choosing a production design. |
| H4 | Does `Accept=yes` work for INET as a control? | `ListenStream=127.0.0.1:0`/fixed high port with `Accept=yes`; connect twice | Instance count | That the directive itself works; separates "directive unsupported" from "AF_UNIX unsupported". |
| H5 | Do `SO_PEERCRED` uid/gid/pid identify the peer, and can a socket be restricted by mode/group? | A small C or Python listener printing `SO_PEERCRED`; connect as root, as the unprivileged user, and (with `setpriv`/`runuser`, if available) as `nobody` to model a user outside the socket group | uid/gid/pid seen for each; connection result outside the group; any tool that was unavailable | Kernel-reported peer identity and filesystem access control. Not an authorization design. |
| H6 | Do state-file ownership and permissions behave as designed? | Create a 0700 dir with a 0600 file, a `flock` holder and a second concurrent writer; run as root and as the unprivileged user | Who can read/write; whether `flock` serializes; atomic rename behaviour | Permissions and locking primitives. Not key lifecycle or attestation (R-10). |
| H7 | Do helper sandbox directives start a unit that only reads what it should? | Unit with `ProtectSystem=strict`, `ProtectHome=yes`, `PrivateTmp=yes`, `PrivateDevices=yes`, `NoNewPrivileges=yes`, empty `CapabilityBoundingSet=`, `ReadOnlyPaths=`, `InaccessiblePaths=`; run `id` and path probes | Startup success; `systemd-analyze verify` warnings; effective access | That the directives are accepted and their local effect. |
| H8 | Containers and Docker API | Do not install Docker. Record whether a Docker socket exists; if it does, only inspect permissions read-only | Presence, owner, mode | Presence only. Container socket-mount delivery and helper Docker API access remain **not demonstrated**. |
| H9 | Hosts without systemd (Unraid-style) | Not testable here | — | Mark **requiring conventional Linux validation**; describe what the helper would need (supervisor, socket path, install path) as options only. |

## 8. Experiment matrix — R-9 (skill-runner isolation)

Create a disposable probe unit with the design's directives, substituting the unprivileged WSL user for
`domovoi-skill` and recording that substitution. Use a probe script that reports `id`, groups, and the
result of each access attempt; do not infer from `systemctl cat`.

| ID | Question | Method | Record | What it proves |
|---|---|---|---|---|
| S1 | Does `User=` plus empty `SupplementaryGroups=` remove group-based access? | `id` inside the unit; compare with the invoking user | uid, gid, groups | The identity the process actually runs as. |
| S2 | Can the skill read `/etc/domovoi/*`? | With `TemporaryFileSystem=/etc/domovoi:ro` and `InaccessiblePaths=`, attempt `ls`/`cat` of a marker file placed in a test `/etc/domovoi` (only if absent) or in the bound test root | Exact `EACCES`/`ENOENT`; whether `systemd-analyze verify` warned about a missing path | The effective view for this unit on this systemd. A missing path makes `InaccessiblePaths` a no-op — test it. |
| S3 | Can it read `/var/lib/domovoi-agent`? | Same, with a 0700 root-owned test directory at that path if absent | Access result | The effective view. |
| S4 | Can it read `/var/lib/domovoi-helper` (key and state)? | Same, 0700 root-owned directory and a 0600 dummy key-shaped file | Access result | The effective view. Not key lifecycle. |
| S5 | Can it reach the helper and skill-runner sockets in `/run`? | If the real socket paths are absent, create 0660 sockets there owned by root and a test group; attempt `connect` and `stat`; also test a socket mode 0666 as a contrast | Connect result, error | Whether the unit's `/run` view and group membership block the socket. |
| S6 | Can it write to skill and package sources? | Read-only bind of a test root; attempt create/append/rename/delete; attempt symlink and hardlink | Exact errors; whether `RestrictSUIDSGID`/`NoNewPrivileges` block anything extra | Write refusal for the tested layout. |
| S7 | Can it read other secret-bearing paths? | Probe `/root`, `/home`, `/proc/1/environ`, `/etc/shadow`, the agent key if one exists | Access result | The unit's reach beyond the named paths. |
| S8 | Does `RuntimeMaxSec` terminate an overrun? | Unit with `RuntimeMaxSec=5s` and a 60-second probe; measure exit and `systemctl show -p Result` | Elapsed time, `Result`, kill signal | Timeout enforcement. |
| S9 | Does closing the connection terminate the runner instance? | With `Accept=yes`, connect, start a child, close the connection; observe the unit and child | Whether the process group exits; time to exit | The design's EOF-cancellation assumption, if H3 shows `Accept=yes` works. Otherwise test the alternative's cancellation. |
| S10 | Can per-package narrowing be expressed per instance? | Instance unit with `BindReadOnlyPaths=/pkg/%i` or equivalent; two instances with different package roots | Whether the specifier resolves; effective view for each instance | Feasibility of per-package narrowing, not a chosen design. |
| S11 | What happens in containers or without systemd? | Not testable; inspect only | — | Mark **requiring conventional Linux validation**. |

Expected observations, to be confirmed or refuted rather than asserted: `Accept=yes` may be rejected for
AF_UNIX on this systemd version (the control row H4 separates that from the directive being unsupported);
`InaccessiblePaths=` against a path that does not exist may be silently ineffective, which is why S2–S4
create the fixture only after checking absence; a mode-0666 socket may be connectable even from outside
the intended group; and `RuntimeMaxSec=` should kill the whole unit rather than only its main process.
Record the actual result for each; do not adjust the design text in this prompt.

## 9. Ordered instructions for the executor

1. Verify the checkout and environment: `git status --short --branch`, branch
   `spike-helper-and-skill-runner-platforms` from the latest `overhaul-investigation`; record `uname -r`,
   `systemctl --version`, `systemd-analyze --version`, `id`, `sudo -n true`.
2. Read `03 RW-39`, `03 RW-37` (state/key permissions only), `04 §8.3–8.4`, `99` R-3/R-9/D-24/D-29/O-30,
   and `H-wsl2-research-environment.md`.
3. Establish the baseline: check whether `/etc/domovoi`, `/var/lib/domovoi-agent`,
   `/var/lib/domovoi-helper`, `/run/domovoi-helper.sock` or `/run/domovoi-skill-runner.sock` already
   exist. If a real installation exists, stop and report.
4. Run the R-3 matrix H1–H7 first; record raw output for each. H3 is the decisive one: whether `Accept=yes`
   is valid for a Unix socket on systemd 259.5.
5. Run the R-9 matrix S1–S10 with a disposable unit. For each path, attempt the access as the unprivileged
   user and record the exact error; never infer isolation from the unit file.
6. Repeat the two or three most important checks with `systemd-analyze verify` and with a second unit
   scope (system vs user) where possible.
7. Record environment and substitution caveats beside every finding: which uid stood in for
   `domovoi-skill`, which paths were test fixtures, what the real install would use.
8. Separate observations from conclusions. Write the report with R-3 and R-9 as separate parts.
9. List platform support options (supported, degraded, unsupported) as recommendations, not decisions,
   and name the evidence each one relies on.
10. Do not update `99`, ADRs, `06`, `07` or the architecture. Do not install anything persistent.

## 10. Report requirements

File: `docs/rework/investigations/J-r3-r9-helper-and-skill-runner.md` (next free letter if taken).

Required sections:

1. **Environment and date** — systemd version, kernel, users, sudo availability, unit scope.
2. **Safety statement** — units, users, groups, directories and sockets created and removed; confirmation
   that no real installation or configuration was touched.
3. **R-3 findings** — one subsection per H row, each labelled and with the exact commands and outputs.
4. **R-9 findings** — one subsection per S row, each labelled; state the substitution used for
   `domovoi-skill`.
5. **The `Accept=yes` question** — the measured answer for AF_UNIX, what it implies for the design, and
   the alternatives it leaves open. Do not select a design.
6. **Platform matrix** — conventional distro with systemd (several versions), container deployment with
   socket mounts, non-systemd supervisors such as Unraid, and WSL2. For each: what was demonstrated, what
   is required, and whether the honest status is supported, degraded or unsupported.
7. **Remaining security assumptions** — what the tests did not prove: real id separation, key secrecy,
   attestation, Docker API isolation, container escape, host filesystem variation, supplementary groups
   in the real units.
8. **Non-acceptance statement** — R-3 and R-9 stay open; O-30 unanswered; no production support claim.

## 11. Completion criteria

- H1–H9 and S1–S11 are attempted or explicitly marked not demonstrated with a reason.
- The `Accept=yes` question has a measured answer, including the exact failure if unsupported.
- Every isolation claim is backed by an access attempt and its exact error, not by a unit-file quote.
- The report has separate R-3 and R-9 parts, a platform matrix, and the residual security assumptions.
- No product file, unit under `deploy/`, installer, config or owner decision was changed.

## 12. Open questions this prompt must not close

- Which distributions and systemd versions are supported, and what the minimum version is.
- Whether a helper can be delivered on non-systemd hosts, and whether those hosts are supported,
  degraded or unsupported.
- Container deployments: socket-mount semantics, group ids, and Docker API access from the helper.
- O-30: the handling of `run_as` values other than `domovoi-skill`.
- Whether per-package filesystem narrowing is feasible on supported platforms.
- Attestation and key lifecycle (R-10), which this prompt deliberately does not test.

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

For this research prompt, read that report as follows: "implemented behaviour" means the evidence and
artefacts produced (units, probes, observations, report); "tests added" means the probe scenarios;
"database and protocol changes" must be "none"; "security" means how the isolation and cleanup rules were
respected during the experiments. Do not invent product behaviour to satisfy the wording.
