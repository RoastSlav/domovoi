# 56 — R-8 effective systemd unit identity research

| | |
|---|---|
| Branch | `spike-systemd-unit-identity` |
| Milestone / research item | R-8 (effective-configuration identity for systemd units); validation gate for M8 `unit.restart` and prompt 45 |
| Dependencies | none. Read `03 RW-34` (stable target identity), `03 RW-23` (guardrails), `04 §8.3` (`[systemd] restart_units`), `03 RW-08` (one execution boundary), `99` R-8 |
| Can run alongside other research | yes: 53 and 54 are independent; 55 is separate and security-sensitive |
| Produces | R-8 evidence: `docs/rework/investigations/K-r8-systemd-unit-identity.md` (next free letter if `K` is taken) |
| Launch gate | Ubuntu WSL2 available (`investigations/H-wsl2-research-environment.md`); no other gate |
| Suggested model | small |

## 1. Objective

Answer one question with measured evidence: **how can Domovoi compute a stable identity for a systemd
unit's effective configuration — its main fragment plus every relevant drop-in — such that an approval
for `unit.restart` is invalidated when, and only when, the configuration that will actually run changes?**

This is research. The deliverable is a report and disposable probe units. Nothing here implements
`unit.restart`, chooses the final algorithm, or accepts R-8.

## 2. What this prompt is not

- It is not prompt 45 (`policy-permit-autonomy`) and not the helper. No `systemd.write` group, no
  `unit.restart` contract, no product code.
- It does not change `deploy/systemd/` units, `install.sh`, `crates/domovoi-checks/` or any product
  configuration.
- It does not accept R-8 or record a decision in `99`. The owner does that after reading the evidence.

## 3. Architecture in plain language

The target identity for a unit is (RW-34):

- `unit:{host}/{canonical_id}` **plus** `config_hash`, which is a SHA-256 over the unit's `FragmentPath`
  contents and every `DropInPaths` file, as `(path, content)` pairs sorted by path.
- Valid fragment locations are `/etc/systemd/system`, `/usr/lib/systemd/system` and
  `/lib/systemd/system`. Generated units under `/run/systemd/generator*` and transient units are **not**
  valid mutation targets in v1.
- Any change to the fragment or a drop-in invalidates the identity; so does an alias that now resolves to a
  different unit. Not bound: `EnvironmentFile=`, `ExecStart=` binaries, container image content — "the
  approval card says what is bound".
- If the identity no longer resolves to the bound value, the step is refused with `target_changed` and
  needs a new approval. Inventory refresh never changes an approved plan. The same binding feeds plan-hash
  v2.

`unit.restart` (M8) is the first privileged contract that uses this: the helper's `systemd.write` group
restarts only units in `helper.toml`'s `[systemd] restart_units`, by canonical unit id plus config hash,
and the agent refuses when the unit's target identity changed. R-8 exists because the identity has not
been measured: which files systemd actually reads, how drop-ins override each other, what `FragmentPath`
and `DropInPaths` report for aliases, masks and templates, whether a restart uses the loaded or the
on-disk configuration, and how wide the race between reading the identity and restarting is on supported
systemd versions.

## 4. Repository and environment context

Current state (verify before relying on it):

- **Nothing computes unit configuration identity.** `crates/domovoi-checks/src/units/` reads runtime
  state only: `systemctl list-units --state=failed` and `systemctl show` for `ActiveState`, `SubState`,
  `NRestarts` and `Result` (`units/failed.rs`, `units/watch.rs`, `commands.rs`). There is no
  `FragmentPath`, `DropInPaths`, `systemctl cat`, config hash, `TargetIdentity` or `unit:` handling in
  `crates/`; grep finds those words only in `docs/`.
- `crates/domovoi-checks/src/commands.rs` runs systemctl through fixed templates: absolute
  `/usr/bin/systemctl` or `/bin/systemctl`, no `PATH` lookup, a 15-second timeout, a 2 MiB output cap, and
  typed slots. `UnitName` is a charset slot `[A-Za-z0-9@._:-]{1,128}` with no canonicalisation and no
  suffix normalisation (`plex` versus `plex.service`), and no template-instance handling.
- `deploy/systemd/` contains the hub and agent units; the single-box drop-in is
  `deploy/systemd/single-box.conf`. None of them is a test target.
- There is no `unit.restart` contract, unit allowlist or `protected_units` config in code; those exist in
  `03`/`04` only.
- Environment: `docs/rework/investigations/H-wsl2-research-environment.md` records Ubuntu 26.04.1 LTS,
  kernel `6.18.58.1-microsoft-standard-WSL2`, systemd 259.5 as PID 1, `systemctl is-system-running` =
  `running`. That is the only systemd version available here.
- The design text is `docs/rework/03-architecture-decisions.md` RW-34 and RW-23, and
  `docs/rework/04-target-architecture.md` §8.3.

Proposed outputs of this prompt, and nothing else:

- `docs/rework/investigations/K-r8-systemd-unit-identity.md` — the report.
- Disposable probe units and scripts, kept in the report or under
  `docs/rework/investigations/probes/`. Do not add product code or change `deploy/`.

## 5. Evidence discipline

Label every finding as exactly one of:

- **demonstrated on WSL2** — observed here, with command and output;
- **not demonstrated** — untested or inconclusive;
- **requiring conventional Linux validation** — depends on another systemd version, distribution layout,
  container runtime or non-systemd host.

Rules:

- A successful observation is evidence, not acceptance. Never write "R-8 is resolved" or "the unit
  identity algorithm is verified on supported distributions".
- Record the environment for every run: `uname -r`, `systemctl --version`, `systemd-analyze --version`,
  whether the unit is system or user scope, and the exact file paths used.
- Separate "the properties report X" from "a restart actually executed X". The second is what M8 needs.
- Do not rely on file mtimes, inodes or `systemctl cat` formatting as identity inputs without measuring
  their stability.
- Commands in §7 are reference sequences: verify their syntax and identifiers before running them, and
  record the command you actually ran.

## 6. Safety and isolation

Mandatory:

1. Use disposable units only, named `domovoi-probe-*`, in `/run/systemd/system` where possible so they
   disappear on reboot. Never edit or restart `domovoi-hub.service`, `domovoi-agent.service` or any other
   real unit.
2. Remove every unit, drop-in, alias, link, mask, transient unit and directory you create; run
   `systemctl daemon-reload` after cleanup and confirm the probe units are gone.
3. Do not reboot, do not stop systemd, and do not change the default target.
4. Do not create units in `/usr/lib/systemd/system` permanently if `/etc` is writable; if a search-path
   test needs a second location, use `/etc/systemd/system` and a temporary copy under
   `/run/systemd/system` first, and record why. Remove anything you place outside `/run`.
5. Transient units via `systemd-run` are allowed for the transient case only; give them `domovoi-probe-`
   names and a short `--property=RuntimeMaxSec=`.
6. Do not install packages or change systemd configuration (`/etc/systemd/system.conf`, logind, etc.).
7. If `sudo` is unavailable, stop and report.

## 7. Experiment matrix

Run every row. `U1`–`U12` are the minimum; add rows when an observation raises a question.

| ID | Question | Method | Record | What it establishes |
|---|---|---|---|---|
| U1 | Baseline | `uname -r`, `systemctl --version`, `systemd-analyze --version`, `ls -l /lib /usr/lib`, `systemctl list-units --type=service --no-pager` (first screen only) | Versions; whether `/lib` is a symlink to `/usr/lib` | The environment only. |
| U2 | What do `FragmentPath` and `DropInPaths` report for a simple disposable unit? | Create `/run/systemd/system/domovoi-probe.service` with `ExecStart=/bin/sleep 300`; `daemon-reload`; `systemctl show -p Id,Names,FragmentPath,DropInPaths,NeedDaemonReload,LoadState domovoi-probe.service`; `systemctl cat` | Exact values; separator and ordering of `DropInPaths`; whether it is empty or lists paths when none exist | The property shape on systemd 259.5. |
| U3 | How do drop-ins override, and in what order? | Add `/run/systemd/system/domovoi-probe.service.d/10-a.conf` and `20-b.conf` that set the same `Environment=`/`ExecStart=`; read `systemctl show`; reverse the numeric prefixes; use an empty assignment to reset a list | Effective value after each change; `DropInPaths` order; whether a later file wins; whether empty assignment resets | Drop-in precedence and ordering on this version. |
| U4 | Which search path wins when a unit exists in more than one location? | Place the unit in `/etc/systemd/system` and a copy in `/run/systemd/system` (or `/usr/lib` if authorised for the test); `daemon-reload`; read `FragmentPath` and `systemctl cat`; remove one | Which path is reported and which content runs; whether the other file is ignored or merged | Search-path precedence. Not every distribution's layout. |
| U5 | Does a restart use the loaded configuration or the file on disk? | A probe script writes a marker naming the argument it was started with; change the unit's `ExecStart`; read `NeedDaemonReload`; restart **without** `daemon-reload`; inspect the marker; then `daemon-reload`, restart, inspect again | Which configuration ran before and after `daemon-reload`; `NeedDaemonReload` values | The loaded-versus-disk distinction and whether `restart` implies a reload (it should not; confirm). |
| U6 | What do aliases, links and masks report? | Add `Alias=domovoi-probe-alias.service` in `[Install]` and `systemctl enable`; `systemctl link` a unit file outside the search path; `systemctl mask domovoi-probe-masked.service`; read `Id`, `Names`, `FragmentPath`, `LoadState` for each; restart through an alias | Property values for alias, linked and masked units; whether restarting the alias runs the canonical unit | Identity/reporting for these cases. |
| U7 | How do template instances resolve? | Create `domovoi-probe@.service`; start `domovoi-probe@one.service`; add a template drop-in and an instance drop-in; read `FragmentPath` and `DropInPaths` for the instance | Template fragment path; which drop-ins apply to the instance | Template behaviour on this version. |
| U8 | What do transient and generated units report? | `systemd-run --unit=domovoi-probe-transient --property=RuntimeMaxSec=5 /bin/sleep 30`; read `FragmentPath`, `DropInPaths`, `LoadState`. For generated units, state that none were produced here unless you can create one without changing system configuration | Property values | Whether the RW-34 exclusion of transient/generated units is enforceable by inspecting these properties. |
| U9 | Are paths and symlinks normalised, and can the same content be counted twice? | Use a symlink outside the search path, a linked unit, and `/lib`/`/usr/lib` where `/lib` is a symlink; compare `FragmentPath` with `realpath`; compute a hash over the reported paths | Whether reported paths are canonical; whether `/lib` and `/usr/lib` produce duplicate entries | Path normalisation needed before hashing. |
| U10 | Is a hash over `(path, content)` deterministic and stable? | Compute SHA-256 over sorted `(path, content)` pairs for the fragment and all drop-ins; repeat after `daemon-reload`, after a no-op `touch`, and after moving a drop-in between prefixes with identical content | Hash stability across no-op changes; hash change on content change; effect of reordering paths | Determinism of the proposed inputs, and which changes do not alter them. |
| U11 | How wide is the race between reading the identity and restarting? | Read the hash, then change a drop-in and `daemon-reload`, then restart; repeat with the edit between two reads; record whether the restart ran the new configuration while the approval-bound hash was old; measure the time window | Observed mismatch; the operations that can occur inside the window | That a read-then-restart sequence is not atomic, and which guard is needed (re-read immediately before, refuse on `NeedDaemonReload`, or both). |
| U12 | Which identity algorithm is supported by the evidence? | From U2–U11, compare at least: (a) RW-34's fragment+drop-ins content hash; (b) the same plus the `DropInPaths` set and `NeedDaemonReload`; (c) hashing `systemctl cat` output; (d) hashing selected `systemctl show` properties | For each: what it detects, what it misses, false invalidations, stability, implementation cost | A recommendation with evidence. It is not an accepted decision. |

Expected observations, to be confirmed or refuted rather than asserted: `DropInPaths` is a list that
includes every applied drop-in; a restart without `daemon-reload` likely runs the previously loaded
configuration; a masked unit reports a fragment under `/dev/null`; a transient unit has no
`FragmentPath`; `/lib` and `/usr/lib` may name the same file on a merged-`/usr` system; and a content
hash is stable across `touch` and `daemon-reload` but changes when a drop-in moves between prefixes
because the path is part of the input. Record what actually happens.

## 8. Failure and recovery scenarios

Record the exact behaviour and what `unit.restart` would have to do:

1. Fragment edited, no `daemon-reload`, restart requested: which configuration runs?
2. Drop-in added, no `daemon-reload`, restart requested.
3. Drop-in removed between the hash read and the restart.
4. Unit masked after approval but before restart.
5. Unit renamed or alias retargeted after approval.
6. Unit file becomes unreadable (permissions) between read and restart.
7. `systemctl show` fails or times out; the identity cannot be computed.
8. Template instance created after approval (no prior target).
9. Transient unit with no fragment presented as a restart target.

Expected shape, to be confirmed rather than assumed: any of these must end as a refusal
(`target_changed`, `unavailable` or `invalid_target`) rather than a restart against an unverified
configuration. Record which of them the proposed identity would actually catch.

## 9. Ordered instructions for the executor

1. Verify the checkout and environment: branch `spike-systemd-unit-identity` from the latest
   `overhaul-investigation`; `git status --short --branch`; record versions and unit scope.
2. Read `03 RW-34`, `03 RW-23`, `04 §8.3`, `99` R-8, and `H-wsl2-research-environment.md` §1–§2.
3. Establish the baseline: list `/etc/systemd/system`, `/run/systemd/system`, `/usr/lib/systemd/system`;
   confirm no real `domovoi-*` unit will be touched; record whether `/lib` is a symlink.
4. Run U2–U4: properties, drop-in order, search-path precedence.
5. Run U5: the loaded-versus-disk behaviour with a marker file. This is the most important row.
6. Run U6–U9: aliases, links, masks, templates, transient units, path normalisation.
7. Run U10–U11: hash determinism and the read-to-restart race.
8. Run U12: compare the candidate algorithms against everything observed and write the recommendation.
9. Clean up every probe unit, drop-in, alias, mask and directory; `daemon-reload`; confirm.
10. Write the report; separate observations from conclusions; list the conventional validation matrix.
    Do not update `99`, ADRs, `06`, `07`, the checks crate or the deployment units.

## 10. Report requirements

File: `docs/rework/investigations/K-r8-systemd-unit-identity.md` (next free letter if taken).

Required sections:

1. **Environment and date** — systemd version, kernel, unit scope, `/lib` layout, sudo availability.
2. **Safety statement** — units and files created and removed; confirmation that no real unit was touched.
3. **Findings by experiment** — one subsection per U row, each labelled, with commands and output.
4. **Loaded versus on-disk configuration** — the measured answer and its consequence for approval
   binding. Include whether a restart implicitly reloads (it should not).
5. **Path normalisation and hashing** — what must be canonicalised, what must be deduplicated, what the
   hash must include, and what it must not include.
6. **Race analysis** — the window, what can change inside it, and the guard or guards the evidence
   supports (re-read, refuse on `NeedDaemonReload`, restart by id only after a verified hash, or a
   combination).
7. **Algorithm recommendation** — a table of the candidate algorithms and their measured properties.
   Recommendation only, with the evidence for each claim.
8. **Conventional Linux validation matrix** — distribution, systemd version, `/lib` layout, drop-in usage,
   container host, non-systemd host, SELinux/AppArmor where relevant, and the specific experiment to
   repeat.
9. **Remaining unknowns and non-acceptance statement** — R-8 stays open; `unit.restart` stays gated; no
   supported-version claim is made.

## 11. Completion criteria

- U1–U12 are attempted or explicitly marked not demonstrated with a reason.
- The loaded-versus-disk behaviour is shown with a marker file and an exact command sequence.
- `FragmentPath` and `DropInPaths` behaviour is recorded for plain, alias, linked, masked, template and
  transient units.
- The report states which changes the proposed hash detects, which it misses, and which produce false
  invalidations.
- A recommendation is given with evidence, and explicitly not recorded as a decision.
- No product file, unit under `deploy/`, checks code or owner decision was changed.

## 12. Open questions this prompt must not close

- Which systemd versions and distributions are supported, and whether the minimum version can be stated.
- Whether aliases and linked units are valid mutation targets at all in v1.
- Whether `EnvironmentFile=` and `ExecStart=` binaries should stay outside the binding (the design says
  they do; the evidence may show a reason to revisit that).
- The exact re-read/refusal point before dispatch, which belongs to the `unit.restart` implementation.

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
artefacts produced (probe units, observations, report); "tests added" means the probe scenarios;
"database and protocol changes" must be "none"; "security" means how the isolation and cleanup rules were
respected during the experiments. Do not invent product behaviour to satisfy the wording.
