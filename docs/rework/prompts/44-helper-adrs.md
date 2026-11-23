# 44 — ADRs for the three identities and helper attestation

| | |
|---|---|
| Branch | `helper-adrs` |
| Milestone | H1 (first prompt); unblocks 47 |
| Dependencies | 54 and 55 evidence merged into `overhaul-investigation` (`J-r3-r9-helper-and-skill-runner.md`, `L-r10-safety-attestation.md`). Read `03 RW-37`, `RW-39`, `RW-12`, `RW-26`, `04b-attestation-encoding.md`, `99` D-24, D-28, D-29, D-31 and R-3, R-9, R-10, and `docs/adr/0018`, `0021`, `0022`, `0028`, `0029` |
| Produces | three ADRs under `docs/adr/` (next free numbers, expected 0030 to 0032) and a list of decisions the owner must take |
| Launch gate | 54 and 55 reports exist, or their missing items are recorded as unproven (the index allows either) |
| Suggested model | mid-size; the final review stays with the main session |

## 1. Objective

Write the architecture decision records that prompt 47 will implement against. They fix, in one place each:

1. the **three identities** (control agent, skill runner, root helper) and what each may reach;
2. how the **helper and the skill runner are delivered and isolated** on each supported platform;
3. the **safety attestation**: its encoding, its trust rules and its fail-closed behaviour.

This is documentation only. No code, no migration, no wire change, no config key.

## 2. What this prompt is not

- It does **not accept R-3, R-9 or R-10**. The evidence is partial (user scope plus one system-scope pass, a scratch harness). The ADRs must say what is proven and what is not, and 47 stays blocked on acceptance.
- It does not edit `03`, `04`, `04b`, `06` or `99`. Where an ADR needs a text change there, list it under "Follow-up edits for the owner" and do not make it.
- It does not decide the items in §6. It states them as open questions with the options and the cost of each.
- It does not implement the helper, the skill runner, `ExpectedSafety` changes or any installer change (that is 47).

## 3. Evidence you must cite

Cite the report section for every claim, and label it exactly as the reports do: *demonstrated on WSL2*, *not demonstrated*, *requiring conventional Linux validation*.

**From 54 (`J-…`):**
- `Accept=yes` is valid on AF_UNIX on systemd 259.5 (one instance per connection, `SO_PEERCRED` reports the client). Cancellation on EOF is **not** automatic. The instance specifier `%i` is `counter-pid_uid-…`, not a package id, so per-package narrowing cannot take its package from the instance name. Only 259.5 was tested.
- Socket `SocketUser/SocketGroup/SocketMode` works as a first layer (system scope). A `0666` socket is **not** protected by the sandbox directives.
- `InaccessiblePaths`, `TemporaryFileSystem`, `BindReadOnlyPaths`, `ProtectSystem=strict`, `ProtectProc=invisible` mask the agent state, helper state and key, and the helper socket, even when the group is granted. Mask order and parent masks matter (S10).
- `SupplementaryGroups=` empty does **not** drop the account's own groups. The real `domovoi-skill` user must belong to no supplementary group. Real `domovoi-agent` and `domovoi-skill` users were never created, so this is untested with them.
- Root-owned 0700/0600 state, the key and `flock` behave as designed against other uids; atomic rename holds; power-loss durability is not shown.
- Docker: absent in WSL, nothing demonstrated. Containers with socket mounts and non-systemd hosts (Unraid): nothing demonstrated.
- `run_as` naming mismatch: `crates/domovoi-core/src/skill/manifest.rs` docs say `domovoi-skills`, D-29/RW-39/D-31 say `domovoi-skill`.

**From 55 (`L-…`):** every item G-1 to G-20 in §15, and the "not demonstrated" list in §16 and §17. The ones that need an ADR position are in §6 below.

## 4. The ADRs to write

Use the format of `docs/adr/0029-package-relay-boundary.md`: Status, Date, Context, Decision, "What the checker enforces", "What the checker cannot see", plus a **"Unproven"** section (new, mandatory here). Status is `proposed` for all three, because R-3, R-9 and R-10 are open. Each links the reports it relies on and the ADRs it extends.

### ADR A: three identities (D-29, D-31, RW-39)

- The three identities, their OS users, what each owns and what each must not reach (agent state, helper state and key, helper socket, skill sources, `/etc/domovoi`).
- Who may connect to which socket and how it is checked (mode, group, `SO_PEERCRED`), and that these are separate layers.
- `run_as`: only `domovoi-skill`; the skill runner refuses anything else (D-31); `Unavailable(run_as_unsupported)`; `domovoi agent selftest` lists affected skills. Fix the `domovoi-skill` versus `domovoi-skills` spelling as a follow-up edit, not here.
- The requirement that `domovoi-skill` has no supplementary groups, and how the installer and selftest verify it.
- The helper allowlist bounds operations; it is not execution authorization (D-29).
- Architecture rules to add for 47 (which crate may name which socket, which directory may construct helper requests), stated as proposed `arch-check` rules, not implemented.

### ADR B: helper and skill-runner delivery and isolation (R-3, R-9)

- The systemd unit shape for the helper and the skill runner: sockets, `Accept=` choice, sandbox directives, `RuntimeMaxSec`, state directory layout, lock and atomic-write rules. State which `Accept=` the design uses, and why, given the 54 findings on EOF cancellation and `%i`.
- Per-package narrowing: say it is infeasible through the instance name under `Accept=yes`, and what the design does instead or defers.
- The platform matrix: systemd hosts, containers with socket mounts, non-systemd hosts. For each: supported, degraded, or unsupported until measured. Containers and Unraid default to **no skill execution and no helper** until a design exists (R-9 text in `99`).
- Docker access from the helper (D-24): record the requirement for its own narrow, first-party design, mark it **not demonstrated**, and keep `container.restart` unattended blocked. Do not design the Docker API surface.
- Version dependence: every directive result is from systemd 259.5 only; list the minimum-version question as unproven.

### ADR C: safety attestation (R-10, RW-37, `04b`)

- What is signed and by whom, the key lifecycle (init, rotate, recover, retire), pinning and fingerprint enrollment, nonce lifetime and single use, `verify_strict`, domain separation, the `generation` rules and the fail-closed state conditions.
- The encoding as `04b` has it, plus the positions on §6 below once the owner has decided. Until then the ADR records each as an open question and does **not** invent a grammar.
- The `expected_safety` tag and the unverified human path as RW-37 says; what the hub stores, what it never trusts.
- Hub-side rule that a correctly signed `complete=false` report is never accepted as verified (55 §7).
- Route-state observation limits (G-7 to G-9, G-17) and that containment stays blocked for configurations the observation cannot represent.
- Known races and the accepted one (G-20).

## 5. Out of scope

- Any code, tests, migrations, wire types, config keys, installer or `deploy/` change.
- Editing `03`, `04`, `04b`, `06`, `99` or the index. Closing or accepting R-3, R-9, R-10.
- Designing the Docker API surface, the non-systemd supervisor, or per-package narrowing.
- Policy, permit, containment (45, 46) beyond naming what they will need from the ADRs.

## 6. Decisions this prompt must put to the owner, not make

For each, write: the question, the options, what each costs, and a recommendation clearly marked as a recommendation. These come from 55 §15 and 54 §7. Group them in one table in each ADR's "Open questions".

1. **G-1** empty `incomplete_reason`: `04b` §1 says a value has one or more fields, RW-37 allows empty.
2. **G-2 to G-5** values and signing when the configuration is invalid or the key is missing: hash value when none exists, `max_ttl_s` and `max_active` in incomplete reports, `generation` in fail-closed reports, and reports that cannot be signed (`key_missing`, `uninitialized`, `key_corrupt`).
3. **G-6** the `init` hash content decides whether the first generation is 1 or 2.
4. **G-10** selector ambiguity for interface names containing `;`; the fix changes `04b`.
5. **G-14** no revocation of earlier verified state by a rejected or incomplete report, or a key change.
6. **G-15** generation repeats under a rolled-back state file are undetectable: accept, or add a hub or helper rule.
7. **G-16** apply-time TTL (refuse or clamp), `max_active = 0`, helper mutation limit.
8. **G-18** wire drift: `PROTOCOL_VERSION` is already 1.2 and `ExpectedSafety` in the proto crate has a free-string reason, no unknown-field refusal and an optional field; the brief for 47 assumes 1.1. Record the actual state; do not change it.
9. `Accept=yes` versus `Accept=no` for the skill runner, and the EOF-cancellation design.
10. Which platforms ship at launch with skills and the helper, given only systemd 259.5 was measured.

## 7. Ordered instructions

1. Read `ARCHITECTURE.md`, the documents in the dependency row, and both reports completely. Verify against the repository the facts in §3 that name code (the `run_as` doc text, `PROTOCOL_VERSION`, `ExpectedSafety`, absence of any helper subcommand, `SO_PEERCRED` and `LISTEN_FDS` in `crates/`). If a fact differs, write the difference first.
2. Create the branch from the latest `overhaul-investigation`.
3. Write ADR A, then B, then C. Commit each separately.
4. Write the "Follow-up edits for the owner" and "Unproven" sections in each; link every claim to a report section.
5. Run `cargo xtask docs-links` if `docs/` book pages link the ADRs, and the repository's markdown checks that apply. No Rust checks are needed.
6. Open the pull request against `overhaul-investigation`.

## 8. Completion criteria

- Three ADRs exist, status `proposed`, each with Decision, checker sections and an Unproven section.
- Every claim about behaviour cites a report section and carries one evidence label.
- No ADR says R-3, R-9 or R-10 is resolved, verified or accepted, and none says a platform is supported without a measurement.
- Every item in §6 appears as an open question with options; none is silently decided.
- The follow-up edits list names each document and section that needs a change.
- No file outside `docs/adr/` changed.

## 9. Open questions this prompt must not close

R-3, R-9 and R-10 acceptance; the §6 items; the helper's Docker API surface; Unraid and container support; per-package narrowing; the `domovoi-skill` spelling change in code.

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
