# 99 — Pre-release cleanup: remove every trace of the old app, then audit code, tests and docs

| | |
|---|---|
| Branch | `pre-release-cleanup` |
| Milestone | closing pass, after every implementation prompt |
| Depends on (must be merged) | every other prompt in the index that is going to ship. Prompts 44–47 are not yet written: if they will land before the first release, run this prompt **after** them. It is a sweep, and it is only worth doing once |
| Produces | no new feature. One new ADR (section 3.5), a single baseline migration, a smaller and truer repository |
| Suggested model | large for the design calls (3.1–3.5) and the final review; medium for the audits; small for the mechanical comment and doc sweeps. Use subagents, one per area, never two on the same files |

## 1. Objective and stance

**This is the first real version of Domovoi.** Nothing the app did before the rework has users, data or
a support promise. So this prompt removes, rather than preserves, everything that exists only to cope
with pre-rework data, behaviour or vocabulary:

- no migration of old rows, no "legacy" label, no drain at start-up, no v1 form of something that now has a v2;
- no comment, doc paragraph, test or snapshot that explains what the app used to do;
- no code kept "for old history".

**What stays is compatibility with the future**, not the past: the schema-version guard that refuses a
database written by a newer build, the exact major.minor protocol rule, pre-migrate backups, and the
upgrade test machinery that will compare *this* release with the next one. Where an item is ambiguous
(it guards a future upgrade *and* mentions the past), keep the guard and delete the past.

Then audit the whole repository for leftovers from the long rework: dead code, stale or wrong comments
and docs, redundant or hollow tests, unreferenced snapshots and fixtures, and text that cites internal
planning documents.

Rule for every deletion: **prove it dead first** (grep, `graphify query`, compiler, a failing test after removal) and record
the proof in the PR. Never weaken a security check, a gate or an arch rule to make a deletion pass: if a gate guards
something that no longer exists, replace it with a smaller gate that guards the absence, and list each such change in the PR.

## 2. Evidence gathered before writing this brief

Verify each item; line numbers will have moved.

- `crates/domovoi-engine/src/legacy.rs` (the one-shot "retire legacy actions" drain) and its suites:
  `crates/domovoi-engine/tests/legacy.rs` (40 mentions), `tests/crash/legacy.rs`,
  `crates/domovoi-hub/tests/hub/legacy_retirement.rs`, `proposal_view.rs`.
- Audit chain "legacy" section: `LegacyDigest`, `legacy_last_id`, `legacy_count`, `legacy_digest` in
  `crates/domovoi-core/src/audit_chain/{encode,verify}.rs`, `crates/domovoi-store/src/{audit_chain,audit_verify}.rs` (about 90 mentions), `crates/domovoi/src/cmd/audit.rs`, migration `0009_audit_chain.sql`.
- Plan-hash and approval **v1 family** kept "for an action created before v2": `crates/domovoi-core/src/action/{request,approval,stored,plan}.rs`, ARCHITECTURE.md (PlanHash paragraph), `docs/book/src/concepts/actions.md`.
- Upgrade fixtures: `crates/domovoi-hub/tests/hub/upgrade_flow.rs`, `crates/domovoi-store/tests/sqlite/upgrade.rs`, the N-1 fixture in the crash harness (`crates/domovoi-testkit/src/crash/`). Check whether they build a database "from before the rework".
- Web: "Legacy" labels, filters and rows in `web/src/features/operations/{OperationsPage,OperationsTable,operationRows}.ts*`, `features/approvals/{ApprovalCard,PlanTag,approvalPlan}`, `web/src/dev/OperationDemos.tsx`, `web/src/test/operationFixtures.ts`.
- Docs that narrate history: `docs/book/src/concepts/actions.md` ("Legacy" banner and page), `docs/book/src/install/upgrading.md` (open-actions section), `concepts/operations.md` ("Actions from before", "the old triage queue", "any more"), `concepts/incidents.md`, ARCHITECTURE.md (the "Legacy action retirement" bullet; sentences such as "held by snapshots frozen from it before its removal", "Nothing creates a legacy action any more"). About 400 lines across `crates`, `xtask`, `web/src`, `docs/book`, ARCHITECTURE.md and TESTING.md contain "no longer", "any more", "used to", "the old", "previously", "formerly" or "was removed".
- Rule breaks already visible: `#[allow(unused_imports)]` at `crates/domovoi-engine/src/operations/investigate/mod.rs:46` (hard rule 5); `crates/domovoi-proto/src/messages/stream.rs:80` says "Prompt 25 consumes them" (committed files never name prompts).
- Citations of internal planning documents in committed files, which will dangle once those documents stay out of `main`: `RW-nn`, `D-nn`, `O-nn`, `R-n`, `docs/rework`, `docs/design-pack`. Found in `web/DESIGN.md` (11), `crates/domovoi-core/src/{capability/target.rs,action/authorization.rs,id.rs,…}`, `crates/domovoi-engine/src/targets/mod.rs`, `xtask/src/arch/rules.rs`, `docs/adr/0021`, `0022`, test files and `migrations/0008_agent_protocol_report.sql`. Re-run the grep from 4.6; this list is a sample.
- Feature gates to confirm still earn their keep: `cfg(feature …)` in `domovoi-checks` (`lib.rs`, `framework/mod.rs`) and `domovoi-hub` (`csp.rs`, `spa.rs`).
- The 15 migrations `0001`–`0015` exist because the schema grew during the rework (for example `0007_advisories_kind_unchecked`, `0006_unit_of_work` rebuilding tables).

## 3. Part A — remove the old app

Do these as separate commits, in this order, each leaving `cargo test` for the touched crates green.

### 3.1 Legacy actions

Decide by reading code what still uses the `actions` table, `ActionRequest`, `ActionRepo`, `ActionHandle`, the legacy approve/deny endpoints and `StateWrite::Action*`.

- Anything that exists only because old action rows may exist is deleted: the drain (`legacy.rs`) and its call at start-up, the `Legacy` status/reason strings ("legacy action expired at upgrade", "legacy executor removed before this run was reconciled"), the read-only legacy rendering in API DTOs, OpenAPI and the web UI, the legacy approve/deny routes.
- If, after that, **no code creates or reads an `actions` row**, delete the table, repositories, store code, testkit fakes, contract suites, `ActionStatus` variants and DTOs too, and say so in the PR. If the plan hash or authorization types are still used by Execution steps, keep exactly those and delete the rest. Do not keep a type "in case".
- The `[orchestration] enabled = false` kill switch is a feature, not compatibility. Keep it, but delete every branch that only existed to let old actions finish while it is off.

### 3.2 Plan-hash and approval v1 family

Keep one plan-hash scheme. Remove the v1 hash, the "v1 approval may have no displayed hash and no provenance" optionality, the `v2` tag in the UI and the docs (a plan code is just a plan code), and the `PlanBinding` variants that exist only for v1. Approval binding columns become non-optional if nothing can leave them empty. Keep INV-1 untouched: `HumanApproval::new` call sites do not change.

### 3.3 Audit chain

The chain starts at row 1 of a new database. Remove the legacy digest and anchor fields (`legacy_last_id`, `legacy_count`, `legacy_digest`), `LegacyDigest`, the "legacy rows" part of `audit verify` and its output, and the start-up pass that digests pre-chain rows. Keep: append-only trigger, `prev_hash`/`hash`, the anchor, `audit verify`, start-up verification, and tamper-detection tests. Update golden vectors and snapshots deliberately.

### 3.4 Configuration, wire and API

- Any config key, alias, default or error message kept so that an older hub config still loads: remove (the exact-key rule "unknown key is an error naming the key" stays).
- Protocol: do **not** change the wire or its version. Remove only shims that translate between 1.2 and an earlier shape, if any exist (`grep -rn "1\.0\|1\.1" crates/domovoi-proto crates/domovoi-agent crates/domovoi-hub`), and every test that exercises a mixed-version path other than the exact-match refusal.
- API: routes, query parameters or DTO fields that only served old data. Cross-check every route against `web/src/api` and the CLI: a route nothing calls and the book does not document as public is a candidate; list candidates, delete the ones that are clearly leftovers, and ask in the PR about any that look like intended public API.
- Regenerate `web/src/api/openapi.json` and `schema.d.ts` (`cargo xtask openapi && cd web && npm run gen:api`); never hand-edit them.

### 3.5 One baseline migration (needs an ADR)

Replace `0001`–`0015` with a single `0001_init.sql` describing the schema as it is now, with no table rebuild steps, no `unchecked` columns, no columns that only held old data, no triggers for tables that no longer exist.

- Build it by dumping the schema a fresh database ends up with after all fifteen, then simplify by hand; add a test that compares the new baseline's schema with the pre-change schema minus the tables and columns this prompt deletes (generate that comparison once, in scratch, and record the result in the PR; do not commit the 15 old files).
- This edits merged migrations, which hard rule 9 and arch-check forbid. That rule exists to protect released databases, and there are none. Write **ADR 0029** (`docs/adr/0029-single-baseline-migration.md`) saying: the rework releases as the first version; the existing history is squashed once; from the first release migrations are immutable. Change the arch-check migration rule in `xtask/src/arch/rules.rs` only as far as the ADR says (for example, the rule compares against the baseline tag of the first release), keep its tests, and add a test that fails if a later edit touches the baseline. This is the only place in the prompt where a rule is allowed to move, and only with the ADR.
- Fresh-database tests (`migrations.rs`, `schema_compat`) are rewritten against the baseline. Development databases made earlier must be recreated; say so in the PR and in `TESTING.md`.
- Keep `schema_compat` (newer-database refusal), `quick_check`, and pre-migrate backup code: they serve the next version.

### 3.6 Vocabulary

After 3.1–3.5, `grep -rniE "legacy|pre-rework|before the rework|older|previously|formerly|no longer|any more|used to|the old |was removed|deprecated|compat" crates xtask web/src docs/book ARCHITECTURE.md TESTING.md README.md CONTRIBUTING.md deploy examples` must leave only hits that are not about the past (for example "backward compatible with Docker API 1.41", "legacy" in an unrelated sense). Review every remaining hit and either rewrite it to say what is true now or delete it.

## 4. Part B — audit and clean

Run each audit, write down the findings, fix them. Items are suggestions of where to look, not a limit.

### 4.1 Dead code (Rust)

- `cargo clippy --workspace --all-targets -- -D warnings` clean; then look for what clippy cannot see: `pub` items with no user outside their crate (make them `pub(crate)` or delete), enum variants, error variants, event kinds, `EffectKind`s, `JournalKind`s, config keys and DTO fields that are never constructed or read, helper functions used only by deleted code, testkit fakes and `Fake*` methods no test calls, port methods with one implementation and no caller.
- Tools to try, if installable on the WSL toolchain: `cargo machete` or `cargo +nightly udeps` for unused dependencies (remove them from `[workspace.dependencies]` too if no crate uses them), `cargo deny check`.
- Remove `#[allow(unused_imports)]` (investigate/mod.rs) by fixing the import. Review the other `#[allow]`/`#[expect]` (runner `spawn.rs`: confirm its justification is documented where the rule allows it).
- Modules that are now a few lines or a re-export of one function; collapse them. Duplicate helpers across `triage/`, `operations/` and `context/` (this is where the queue removal and the layered-context rework met).
- `cfg(feature …)` gates: delete a feature nobody enables or disables.
- Leftover debug output: `eprintln!`, `dbg!`, commented-out code, unused `tracing` fields.

### 4.2 Dead code (web)

- Unused exports (`ts-prune` or `knip` run through `npx`, without adding them to the project unless the owner wants that), unused kit components, icons, tokens, message strings in `web/src/design/messages.ts`, CSS rules, `web/src/dev` demo pages for removed components, unused fixtures in `web/src/test`.
- API client functions with no caller.
- Respect `web/DESIGN.md`: new primitives belong in the kit, but nothing here adds one.

### 4.3 Comments and code docs

For every file touched by the rework (use `git log --stat 224df468~60..HEAD` as a starting list, then the rest):

- Module and item doc comments that describe a design that changed (queue, legacy executor, text protocol `PROPOSALS:`, v1 hash, "behind the flag", "until X lands", "for now", "temporary", "will be").
- Comments that narrate history or the change instead of the code ("now", "no longer", "instead of", "replaces", "was", "moved from").
- Comments that restate the name; doc comments that say a function returns what its type says. AGENTS.md rule: say what the item is for.
- Comments that name prompts, prompt numbers, the build plan, agents, or cite `RW-nn`, `D-nn`, `O-nn`, `R-n`, `§n` of internal documents, or paths under `docs/rework` and `docs/design-pack`. Replace each with the *reason* in plain words, or point at `ARCHITECTURE.md` or an ADR that is committed. Fix `stream.rs:80` first.
- Wrong cross-references: intra-doc links and `[`Type`]` that point to renamed items. Run `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` and fix every warning.
- Public items missing docs (R10); `//!` headers present on every module that has more than one item.
- Error messages and log lines that mention removed features or give wrong advice.

### 4.4 Tests

- Tests for deleted behaviour; tests whose only assertion is that something absent is absent, unless an arch-style gate is cheaper.
- Test names that describe a function instead of a behaviour; test modules with a name from an earlier design (`service_parity`, `legacy`, `proposal_view`, `queued`, "both modes"). The parity suite and its `Mode` enum were scaffolding for a switch that is gone: keep the **frozen expectations** as plain tests, delete the two-mode machinery, rename.
- Duplicated scenarios across `tests/drills`, `tests/services`, `tests/operations`, `tests/crash`, hub `tests/hub` and web page tests: keep the one that asserts the behaviour at the right level, delete the copies. A deletion must not drop an INV-numbered invariant or a case listed in `TESTING.md`; check coverage by mutating (comment out the guarded line, see a test fail) for any non-obvious deletion and note it in the PR.
- Sleeps in tests (`grep -rn "sleep\|Instant::now\|SystemTime" crates/*/tests crates/*/src --include=*.rs`), `#[ignore]`, `it.skip`, `test.todo`, `xit`: fix or delete.
- Snapshots: `cargo insta test --unreferenced=reject --workspace` (or equivalent) lists `.snap` files no test uses; delete them. Review every remaining snapshot header and body for stale vocabulary (untracked new `context/snapshots/` and `tests/services/snapshots/` are in this class: read them).
- Gate tests (`tests/gates.rs` in each crate): read all of them. Strengthen or tighten any whose subject changed; never delete one without the replacement described in section 1.
- Fixtures and data files under `crates/*/tests`, `examples/`, `deploy/test`: unused or describing removed config.
- Testkit: fakes and contract suites for removed ports.

### 4.5 Documentation

Make every document describe **what is true now, in present tense, without history**.

- `ARCHITECTURE.md` (423 lines, one very long bullet per area): rewrite for readability, drop "the queue", "legacy", "any more" and process narration, keep INV, EXEC and MOD rules intact and accurate; check every type name, path, route, rule id and file name against the code; check the crate dependency table against each `Cargo.toml` and against `cargo xtask arch-check`.
- `docs/adr/`: ADRs are historical records and are not rewritten. Do fix: stale `Status`; mark ADRs whose decision was replaced as `Superseded by NNNN` (check 0014, 0019, 0021, 0022, 0024, 0028 against the final design); remove references to internal documents; make sure the README index lists every file and its status.
- `docs/book/src`: read every page against the running product. Concepts pages (`actions.md` is mostly legacy; fold what is still true into `operations.md` and delete the page and its `SUMMARY.md` entry and inbound links), install pages (`upgrading.md`: describe upgrading from this release to the next, not from the old app), reference pages (`configuration.md` is generated: `cargo xtask docs-config`; `api.md`, `cli.md`, `data-model.md`, `protocol.md`, `agent.md`, `backup.md` must match code, routes, flags, tables and messages), guide pages. Run `cargo xtask docs-links`.
- `README.md`, `CONTRIBUTING.md`, `TESTING.md` (every command still works; every path exists; WSL notes are current; test counts and job names match CI), `SECURITY.md`, `web/README.md`, `web/DESIGN.md` (remove the citations of internal ids; kit inventory matches `web/src/components`), `deploy/README.md`, `examples/README.md`, `docs/maintainers/repository-setup.md`, `docs/assets/*.svg` (do they show the current UI?).
- Example files: `examples/hub.toml`, `hub.full.toml`, `agent.toml`, each skill under `examples/skills` and `deploy/**` configs must load against the current config schema and manifest rules. Add or reuse a test that loads them if one does not exist.
- `CHANGELOG.md` is generated by release-plz: do not hand-edit it unless it names a prompt, a build plan or an agent, in which case reword that line only.
- `AGENTS.md`: update the command table, hard rules and paths if any changed in this prompt (for example the migration rule after ADR 0029). Keep it the only committed file that may name agents and prompts.

### 4.6 Repository hygiene

- Final greps over **committed** files, expected to return nothing outside `AGENTS.md`:
  `git grep -nIiE "prompt [0-9]|\bP[0-9]{2}[a-z]?\b|build.plan|coding agent|claude|opencode|\bRW-[0-9]|\bD-[0-9]{2}\b|\bO-[0-9]{2}\b|docs/(rework|design-pack|build-plan)|overhaul-investigation"`
- Files that should not be committed: `git ls-files | grep -E "snap.new|\.orig|\.rej|\.bak|~$|\.log$|smoke-out|dist/"`; check `.gitignore` covers `web/smoke-out`, `web/dist`, `target`.
- Large or binary files nobody references; stale scripts in `web/scripts`, `deploy/test`, `xtask`; unused `xtask` subcommands; unused CI jobs or steps in `.github/workflows` (run `actionlint`); `Dockerfile`/`docker/*`/`install.sh` paths that no longer exist; `release-plz.toml` and `deny.toml` entries for crates or advisories no longer present (`deny.toml` skips and exceptions especially).
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, web `lint`, `typecheck`, `format:check`, `build` (including the bundle-size budget) all green.

### 4.7 Local-only planning documents (not part of the PR)

These are never committed to `main`, but they are stale and will mislead the next session. Do this as a final, separate step and **do not commit** them to `main`'s history (see how they are tracked on the V2 branch: `git ls-files docs/rework docs/design-pack`):

- `docs/rework/07-implementation-prompt-index.md` status line ("Prompts 08–24 and 57 are merged …") and the 06 roadmap readiness; mark done prompts, record that this prompt removed all pre-rework compatibility (add owner decision **D-36**: first release, no compatibility with the pre-rework app, to `99-open-decisions.md`), update the "legacy" rows (RW-32, RW-36, 58/60 wording) to say they were removed.
- `docs/build-plan/README.md` still describes a 27-prompt v0.1 plan; mark it historical.
- `docs/build-plan/domovoi_phase7_deepseek_prompts/`, `docs/rework/phase7-working/`, `docs/design-pack/10-capability-roadmap.md` vs `10-evolution-roadmap.md` (two files with the same number): list duplicates and obsolete files in the report; propose, do not delete.

## 5. Out of scope

- New features, new routes, new screens, new dependencies.
- Wire or protocol changes (no version bump), changes to the approval flow, the policy evaluator, or any security behaviour.
- Rewriting merged ADR decisions.
- Prompts 44–47 (helper ADRs, key enrollment) if they are not merged: do not start them. Where a placeholder for them exists in the UI or docs, leave it and make sure it is labelled honestly.
- Fixing a bug found along the way: note it in the PR description and open it as a follow-up, unless the cleanup cannot proceed without it.

## 6. Order of work and commits

1. Branch from the up-to-date base. Run the full local checks once and record the baseline (`cargo xtask ci --fast` on WSL, web checks, `cargo doc`, `cargo insta test --unreferenced=reject`).
2. 3.1 → 3.2 → 3.3 → 3.4 (each: delete, fix compile, delete the tests of the deleted thing, regenerate generated files).
3. 3.5 baseline migration and ADR 0029.
4. 3.6 vocabulary sweep.
5. 4.1–4.4 audits. Subagents may run the *searches* in parallel; fix in one session so edits do not collide.
6. 4.5 documentation, then 4.6 hygiene greps.
7. Full run of every check named in `TESTING.md` "Before you push". This prompt, unlike the others, **does** run `cargo xtask ci --web` and the Linux-only suites in WSL, because it touches everything.
8. 4.7 local documents.

Commit one concern per commit (`refactor(engine): remove the legacy action drain`, `refactor(store): squash migrations into one baseline`, `docs: describe the product as it is`, `test: remove scaffolding for the retired switch`). One scope each; PR title has at most one scope, for example `chore(repo): remove pre-release leftovers`. The PR description lists, per section above, what was deleted, the proof it was dead, every gate or test removed or replaced and why, and anything left that you were unsure about.

## 7. Acceptance criteria

1. No code, schema, endpoint, UI element, test, snapshot or document exists to support data or behaviour from before the rework (3.1–3.6); the greps in 3.6 and 4.6 are clean.
2. A fresh database is created by exactly one baseline migration; ADR 0029 exists; a test fails if the baseline is edited after the first release.
3. The audit chain, plan hash and approval binding have one form each; the invariants INV-1 (human-only approval), the closed `MetaTool` set, the single dispatch site (`EXEC-2`) and `MOD-*`/`CALLER-1` are unchanged and their gates still pass.
4. `cargo doc` has no warnings; `cargo xtask docs-links` passes; `docs-config` and the OpenAPI/`schema.d.ts` regeneration produce no diff.
5. No unreferenced snapshot, no `#[ignore]`/skip, no sleep in tests, no new `#[allow]`; the pre-existing `#[allow(unused_imports)]` is gone.
6. Every example config and skill loads in a test.
7. Every comment or doc that mentions a prompt, a plan or an internal id is gone from committed files.
8. The workspace is no larger in behaviour and measurably smaller in code: the PR states lines and files removed per crate and the test count before and after, with a reason for each test removed.
9. `cargo xtask ci --web`, the web checks including `npm run smoke`, and the WSL Unix suites pass on the branch.

## 8. Tests required

- `baseline_migration_matches_the_expected_schema` (tables, columns, indexes, triggers; fresh database).
- `baseline_migration_is_immutable_after_first_release` (arch-check rule test for ADR 0029).
- `audit_chain_starts_at_the_first_row_of_a_fresh_database` and the existing tamper, truncation and anchor tests, updated.
- `new_database_has_no_legacy_action_surface` or the equivalent compile-level absence, only if section 3.1 leaves any action type in place.
- `example_configs_and_skills_load` (hub, agent, each skill).
- `every_book_page_is_in_summary_and_links_resolve` already exists as `docs-links`; make sure it runs in CI.
- Existing gate tests: all still present; each one changed or replaced is listed in the PR with the reason.

---

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
  *(This prompt is the one sanctioned exception to the migration rule, and only as section 3.5 and ADR 0029 describe.)*
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
  *(This prompt widens the last bullet group as section 6, step 7 says: it touches every crate.)*

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
