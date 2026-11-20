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
planning documents. Three things get a full pass of their own, not a skim:

- **The book** (`docs/book`) must be up to date, correct and complete (4.5).
- **The ADRs** (`docs/adr`) must each still be needed, still true, and all `Accepted` (4.6).
- **`examples/`** must be checked file by file against the final product (4.7).
- **The README** must be rewritten as a friendly, accurate front page with real screenshots and code-derived diagrams (4.8a).

Finally, **delete the internal planning documents** (`docs/rework/`, `docs/design-pack/`, and the local
`docs/build-plan/`), including this brief, as the very last commit (4.9). Because this brief lives in
`docs/rework/prompts/`, read it completely before you start and do not delete it until everything else is
done and green.

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
- Citations of internal planning documents in committed files, which will dangle once those documents stay out of `main`: `RW-nn`, `D-nn`, `O-nn`, `R-n`, `docs/rework`, `docs/design-pack`. Found in `web/DESIGN.md` (11), `crates/domovoi-core/src/{capability/target.rs,action/authorization.rs,id.rs,…}`, `crates/domovoi-engine/src/targets/mod.rs`, `xtask/src/arch/rules.rs`, `docs/adr/0021`, `0022`, test files and `migrations/0008_agent_protocol_report.sql`. Re-run the grep from 4.10; this list is a sample.
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

### 4.5 The book: up to date, correct and complete

`docs/book/src` is the product documentation and the only documentation a user reads. Bring it to a state where a new owner can install, configure, operate and troubleshoot Domovoi from it alone. Do this by **using the product**, not only by reading code: build the hub with the UI embedded, start it with the dev hub or a single-box install in WSL, click through every page, run every CLI command, and compare.

1. **Coverage matrix first.** Write (in the PR description, not in the repo) a table of every user-visible thing against the page that documents it, and fill each gap with a page or a section. Walk at least: every top-level web page and its controls (Control Room, incidents, operations and the step timeline and inspector, approvals, hosts, skills, capability center and install wizard, policy editor, simulator and publication report, knowledge pages, conversations/operator panel, visibility profiles, audit, retention, settings, graph if shipped); every `domovoi` CLI command and flag; every configuration section and key; every HTTP route that is public API; every built-in check and its thresholds; skills, manifests and the sandbox; webhooks; Discord setup and every card and button; digest; LLM providers and budgets; agents, enrollment, the package/ceiling/helper concepts as shipped (placeholders for unwritten helper features are labelled as not available yet, once, in one place); backup, restore, retention; metrics and readiness; security model and what is *not* protected; upgrading from this release to the next; Docker, binary, container-agent, Unraid and systemd installs; troubleshooting for the failure modes the code can actually report.
2. **Correct.** Every statement, number, default, command line, path, port, file name, error text, button label and screenshot must match the code and the running UI. Copy-paste every command block into a shell and run it. Every config snippet must load. Defaults come from the generated reference (`cargo xtask docs-config`), never typed by hand twice.
3. **Up to date and without history.** Present tense, no "now", "new", "no longer", "instead of the old". `concepts/actions.md` is mostly about the removed legacy path: fold what is still true into `operations.md`, delete the page, its `SUMMARY.md` entry and every inbound link. `install/upgrading.md` describes upgrading from this release to the next (backup, pre-migrate backup, schema guard, protocol rule, hub then agents), not from the old app.
4. **Complete structure.** `SUMMARY.md` lists every page once, in a reading order that works for a first-time reader (introduction, quick start, concepts, guides, install, reference, security, troubleshooting, FAQ); no orphan page; no empty or stub page; each concept page says what it is, why it matters and where to configure it; each reference page is exact and complete rather than illustrative. The FAQ answers the questions the other pages raise, not marketing.
5. **Mechanics.** `mdbook build docs/book` with no warnings; `cargo xtask docs-links` (links, anchors, pages) passes and runs in CI; all code fences have a language; no raw HTML needed; images exist and show the current UI.
6. **Reference pages are checked mechanically where possible.** Extend `xtask` if cheap: for example a check that every public route in the OpenAPI document appears in `reference/api.md`, every CLI subcommand appears in `reference/cli.md`, every table appears in `reference/data-model.md`, every wire message appears in `reference/protocol.md`. If a check is not cheap, check by hand and record how in the PR.

### 4.6 ADRs: needed, true, and all accepted

Every file in `docs/adr/` is read in full against the final code and `ARCHITECTURE.md`. Outcome for each ADR must be one of:

- **Keep**: the decision is still in force and the text is true. Status `Accepted`. Remove citations of internal ids and of the rework documents; fix names of types, paths and rules that changed.
- **Rewrite**: the decision is in force but the text describes a design that changed during the rework (for example the single writer, approval rehydration, plan hash and authorization, execution boundary, capability registry, operation aggregate, use-case layer, policy shadow). Rewrite it to the final design, as a clean decision record: context, decision, consequences. No history of how it got there.
- **Delete**: the decision was replaced, is now obvious from the code, or recorded something that no longer exists (for example any ADR about the removed legacy executor or the retired action writer). Delete the file and fix every link to it. Do not mark an ADR `Superseded`.

When done: **every remaining ADR has status `Accepted`** and none is `Proposed`, `Draft`, `Deprecated` or `Superseded`; `0000-template.md` and `README.md` stay; the README index lists exactly the remaining files with a one-line summary each; numbering gaps from deleted ADRs are fine and are not renumbered (links and arch-check rule citations stay stable), and the README says so in one sentence. Check that each rule id that `xtask/src/arch/rules.rs` cites (R-numbers, INV, EXEC, MOD, CALLER) points to an ADR or to `ARCHITECTURE.md` that still exists. The ADR required for changes to enforced rules (see arch-check "rule changes without an ADR") keeps working. ADR 0029 (section 3.5) is written as a plain decision record: migrations are immutable from the first release and the baseline is the starting point.

Also confirm the set is complete: any lasting decision the code relies on that has no ADR (for example the single-baseline schema, the use-case layer's `Caller`, the closed meta-tool set, the human-only approval rule, the exact protocol rule, the audit chain, the helper boundary as built) gets one, short, `Accepted`. Do not invent decisions that were never made; ask in the PR about any doubtful one.

### 4.7 `examples/`

Check every file under `examples/` against the final product, and make the folder worth shipping:

- `examples/README.md` says what each file is for and how to use it; every command in it works.
- `hub.toml`, `hub.full.toml` and `agent.toml` load without error and without warnings under the current schema; `hub.full.toml` shows **every** section and key (including operations/orchestration, retention, policy-related settings, knowledge, LLM providers and budgets, Discord, webhooks, metrics) with comments that are true and match the generated configuration reference; no key that no longer exists; no secret-looking placeholder that looks real.
- Each skill under `examples/skills/*`: manifest valid under the current manifest rules, declares its capability contract/effect class as the runner and registry require, entry script exists and is executable where needed, argument schema matches what the script reads, sandbox and `run_as` settings are valid (nothing the runner refuses), and the skill appears in the book's skills guide with matching text. Run each skill's own test or drill if it has one; if it has none, add a loader/validation test. Delete a skill that no longer fits the product; add a short one only if a documented feature has no example.
- Anything else in the folder (docker compose files, systemd units) is checked against `deploy/` and the install pages and deleted or fixed if it disagrees; duplicates between `examples/` and `deploy/` are resolved to one place.
- A test (in `domovoi-config`, `domovoi-runner` or `xtask`, wherever the loader lives) loads every example and every skill so they cannot rot; it runs in normal CI.

### 4.8 Other documentation

Make every document describe **what is true now, in present tense, without history**.

- `ARCHITECTURE.md` (423 lines, one very long bullet per area): rewrite for readability, drop "the queue", "legacy", "any more" and process narration, keep INV, EXEC and MOD rules intact and accurate; check every type name, path, route, rule id and file name against the code; check the crate dependency table against each `Cargo.toml` and against `cargo xtask arch-check`. It must not cite the rework documents.
- `CONTRIBUTING.md`, `TESTING.md` (every command still works; every path exists; WSL notes are current; test counts and job names match CI), `SECURITY.md`, `web/README.md`, `web/DESIGN.md` (remove the citations of internal ids; kit inventory matches `web/src/components`), `deploy/README.md`, `docs/maintainers/repository-setup.md`, `docs/assets/*.svg` (do they show the current UI?).
- `CHANGELOG.md` is generated by release-plz: do not hand-edit it unless it names a prompt, a build plan or an agent, in which case reword that line only.
- `AGENTS.md`: update the command table, hard rules and paths if any changed in this prompt (for example the migration rule after ADR 0029, and the removal of `docs/rework` references, if any). Keep it the only committed file that may name agents and prompts. Remove its mentions of `docs/build-plan` and of prompt-pack workflow only if the prompt pack no longer exists after 4.9; keep the rules every future contributor needs.

### 4.8a The README: rewrite it for people

`README.md` is the front door and is out of date: it still says "v0.1, before its first release", its diagram shows only the old hub and agent picture, its two images are placeholders ("Screenshot coming with the first release"), and it describes triage and approvals but not operations, policy, knowledge, conversations, packages or the audit chain. Rewrite it. It stays a **front page, not a manual**: friendly, a little marketing, depth left to the book, but every claim true and every claim shipped.

**Voice.** Plain, warm, second person ("you"), short sentences, concrete nouns. Lead with the problem, then the feature. No jargon without a half-sentence explaining it, no buzzword stacks, no emoji spam. Technical detail is welcome where it earns trust (the lifecycle, the safety measures), written the way you would explain it to a friend who runs a homelab or a small company's servers.

**Structure** (adjust headings, keep the order of ideas):

1. **Title, one-line pitch, badges** that work (CI, docs, license), and the one-sentence "named after the household spirit" line. Remove the stale status banner; state the real release status in one sentence.
2. **Hero visuals.** Real screenshots, not placeholders: a Discord card, the incident page, the operation timeline with an approval card (and the Control Room if shipped). Capture them from the running dev hub with seeded data (`cargo run -p domovoi-hub --example dev_hub`, `cd web && npm run dev`, `node scripts/screens.mjs`), light and dark (`<picture>` with `prefers-color-scheme`), with alt text that says what is shown. Replace the `docs/assets/*.svg` placeholders and delete any asset the README no longer uses. No secret, real host name or address may appear in them.
3. **The problem, in a few lines.** The 3 a.m. disk-full alert that tells you nothing; cron scripts and five dashboards nobody maintains; a chatbot that could `rm -rf` your server. What Domovoi is instead: it watches, works out what happened, tells you in plain language, and does nothing until you say yes.
4. **What it does for you** (the best parts, 6 to 8 items, each a short paragraph with a small concrete example): watching and webhooks; one incident per problem instead of fifty alerts; an AI investigation that reads logs and state (read-only) and writes a short card; Discord threads you can answer in; approvals you control, with the exact plan shown; your own scripts as safe, validated "skills" and installable signed capability packages; a policy you can try in watch-only mode before it ever acts; what it remembers (knowledge, past incidents, search of earlier fixes); the operator chat for asking about a host; configurable retention and the audit log. Include a feature only if it ships in this release; anything unfinished is left out or labelled once as coming later.
5. **See it in action.** Two or three short worked examples written as little stories, with the real text of a card or timeline step taken from the running product or the drills: for example "a container keeps crash-looping at night", "a disk is filling", "a webhook from Uptime Kuma goes down". For each: what happened, what Domovoi showed, what you tapped, what ran. Include the exact command or config for one of them (a skill manifest or a webhook line) so a reader sees how little setup it takes. Examples must run against the shipped build; copy them from `examples/`.
6. **How it works: the lifecycle.** The short technical part, with diagrams **derived from the code, not from memory**: read the enums and state machines (`IncidentStatus`, `OperationStatus`, `StepKind`, `StepOutcome`, `EdgeKind`, effect kinds) and draw what exists. Include:
   - an architecture diagram: hosts with their agent, the hub (web UI, API, incident manager, operation service, SQLite), the LLM, Discord, webhook sources, and the direction of every connection (agents dial out);
   - the **incident lifecycle** (signal, correlate, open, notified, acknowledged or muted, resolved, reopened) as one Mermaid diagram;
   - the **operation lifecycle** (investigate: build context, ask the model, read evidence, question, plan, approval, execution, verification, notify, close, with the statuses as they actually exist) as one Mermaid diagram;
   - under the diagrams, one or two sentences for **each part**: what the agent does, what the hub does, what an incident is, what an operation and a step are, what an effect is and why a restart loses nothing, what a skill is, what the policy does.
   Mermaid must render on GitHub; keep node labels short; preview it before finishing.
7. **How it keeps you safe.** The best measures, each in one or two plain sentences with a link to the security page. Claim only what the code enforces and a test or gate proves; verify each against code before writing. Pick from: only an authenticated human can approve, and nothing the model can call can approve (a closed tool set); what you approve is bound to the exact plan by a hash, so what runs is what you saw; the model only has read-only tools and treats logs and alert text as untrusted; fixes run as argv with validated parameters, never a shell, and are re-checked on the host right before they run; skills and packages run under a separate unprivileged identity and sandbox, as shipped; packages are signed and verified on the host against keys the hub cannot change; destructive actions need typed confirmation on the web and cannot be approved from chat; a policy rule starts in watch-only mode and acts on its own only after you promote it; agents dial out over TLS and a revoked key can never reconnect; secrets are never logged or shown; the append-only audit log is hash-chained so tampering is detectable (`domovoi audit verify`); rate limits and a host-side ceiling; sessions, CSP and a step-up password for risky settings; and the proof: tests, gate tests, an architecture checker and scored drills (prompt injection, decoys, destructive-only fixes) on every change. Add one honest sentence on what Domovoi does not protect against, matching `docs/book/src/security.md`.
8. **Quick start**: the shortest true path (install, open the setup link, first host, first card), commands copied from and verified against `docs/book/src/quick-start.md` and `install.sh`, plus one line each for Docker and the container agent.
9. **Documentation, contributing, license**: links that resolve. Keep the license section's substance; review it for accuracy only and do not change license terms.

**Rules.** About 250 lines at most; no internal ids, no prompt or build-plan words, no "legacy", no history of the rework; no claim without a source in the code or the book; no number that is not generated or checked (test counts, check counts, supported platforms); every link, image and command works; one terminology (hub, agent, incident, operation, step, skill, capability, policy) matching the book. Finish with a stranger's read: can they tell in 30 seconds what it is, what problem it solves and why it is safe to trust? Put before and after screenshots of the rendered README in the PR.

### 4.9 Remove the internal planning documents

Last step, only after everything else is committed and green. These documents were scaffolding for the rework; the product is now described by the book, `ARCHITECTURE.md` and the ADRs, so nothing may depend on them.

1. Before deleting, grep the whole repository (committed and local) for any remaining reference to them: `git grep -nIE "docs/(rework|design-pack|build-plan)|\bRW-[0-9]+|overhaul-investigation"`. Fix each reference first (replace it with the reason in words, or with a link to the book, `ARCHITECTURE.md` or an ADR). Pay special attention to `web/DESIGN.md`, `xtask` rule messages, ADRs, test comments, `.github`, `AGENTS.md` and `CONTRIBUTING.md`.
2. Preserve anything that is still *true and needed* and exists only in those documents: for each of `docs/rework/00`–`07`, `04b-attestation-encoding.md`, `investigations/`, `docs/design-pack/*` and `docs/build-plan/*`, ask "does a contributor or user still need a fact only found here?" If yes, move that fact (not the file) into the book, `ARCHITECTURE.md`, an ADR, or a `docs/` page that reads as product documentation, in present tense. The attestation encoding and golden vectors, if the code relies on a specification only written in `04b`, need a committed home next to the code (a module doc or an ADR) before `04b` goes. List what you preserved and where in the PR.
3. `git rm -r docs/rework docs/design-pack`. Also delete the untracked local `docs/build-plan/` (and `domovoi_phase7_deepseek_prompts/` inside it), `.git-msgs/` if it holds only scratch, and remove their lines from `.git/info/exclude` only if nothing else needs them (`docs/build-plan/` is local-only; deleting it needs no commit, but say so).
4. Re-run `cargo xtask docs-links`, `cargo xtask arch-check` and the final greps of 4.10. They must pass with the folders gone.
5. Commit as `docs: remove the internal planning documents`. Open the PR with a note that the branch policy "internal documents stay out of `main`" no longer needs a promotion filter for them.

### 4.10 Repository hygiene

- Final greps over **committed** files, expected to return nothing outside `AGENTS.md`:
  `git grep -nIiE "prompt [0-9]|\bP[0-9]{2}[a-z]?\b|build.plan|coding agent|claude|opencode|\bRW-[0-9]|\bD-[0-9]{2}\b|\bO-[0-9]{2}\b|docs/(rework|design-pack|build-plan)|overhaul-investigation"`
- Files that should not be committed: `git ls-files | grep -E "snap.new|\.orig|\.rej|\.bak|~$|\.log$|smoke-out|dist/"`; check `.gitignore` covers `web/smoke-out`, `web/dist`, `target`.
- Large or binary files nobody references; stale scripts in `web/scripts`, `deploy/test`, `xtask`; unused `xtask` subcommands; unused CI jobs or steps in `.github/workflows` (run `actionlint`); `Dockerfile`/`docker/*`/`install.sh` paths that no longer exist; `release-plz.toml` and `deny.toml` entries for crates or advisories no longer present (`deny.toml` skips and exceptions especially).
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, web `lint`, `typecheck`, `format:check`, `build` (including the bundle-size budget) all green.

## 5. Out of scope

- New features, new routes, new screens, new dependencies.
- Wire or protocol changes (no version bump), changes to the approval flow, the policy evaluator, or any security behaviour.
- Changing what an ADR decided. Reword, merge or delete ADRs only as 4.6 says; a decision the code does not follow is reported, not edited away.
- Prompts 44–47 (helper ADRs, key enrollment) if they are not merged: do not start them. Where a placeholder for them exists in the UI or docs, leave it and make sure it is labelled honestly.
- Fixing a bug found along the way: note it in the PR description and open it as a follow-up, unless the cleanup cannot proceed without it.

## 6. Order of work and commits

1. Branch from the up-to-date base. Run the full local checks once and record the baseline (`cargo xtask ci --fast` on WSL, web checks, `cargo doc`, `cargo insta test --unreferenced=reject`).
2. 3.1 → 3.2 → 3.3 → 3.4 (each: delete, fix compile, delete the tests of the deleted thing, regenerate generated files).
3. 3.5 baseline migration and ADR 0029.
4. 3.6 vocabulary sweep.
5. 4.1–4.4 audits. Subagents may run the *searches* in parallel; fix in one session so edits do not collide.
6. 4.5 book, 4.6 ADRs, 4.7 examples, 4.8 other documentation, 4.8a README (last of the docs, so it links to final pages and reuses final screenshots), then 4.10 hygiene greps. Write the book coverage matrix before editing pages.
7. Full run of every check named in `TESTING.md` "Before you push". This prompt, unlike the others, **does** run `cargo xtask ci --web` and the Linux-only suites in WSL, because it touches everything.
8. 4.9 remove the internal planning documents, as the last commit, then re-run the checks of step 7 that docs touch (`docs-links`, `arch-check`, `mdbook build`).

Commit one concern per commit (`refactor(engine): remove the legacy action drain`, `refactor(store): squash migrations into one baseline`, `docs: describe the product as it is`, `test: remove scaffolding for the retired switch`). One scope each; PR title has at most one scope, for example `chore(repo): remove pre-release leftovers`. The PR description lists, per section above, what was deleted, the proof it was dead, every gate or test removed or replaced and why, and anything left that you were unsure about.

## 7. Acceptance criteria

1. No code, schema, endpoint, UI element, test, snapshot or document exists to support data or behaviour from before the rework (3.1–3.6); the greps in 3.6 and 4.6 are clean.
2. A fresh database is created by exactly one baseline migration; ADR 0029 exists; a test fails if the baseline is edited after the first release.
3. The audit chain, plan hash and approval binding have one form each; the invariants INV-1 (human-only approval), the closed `MetaTool` set, the single dispatch site (`EXEC-2`) and `MOD-*`/`CALLER-1` are unchanged and their gates still pass.
4. `cargo doc` has no warnings; `cargo xtask docs-links` passes; `docs-config` and the OpenAPI/`schema.d.ts` regeneration produce no diff.
5. No unreferenced snapshot, no `#[ignore]`/skip, no sleep in tests, no new `#[allow]`; the pre-existing `#[allow(unused_imports)]` is gone.
6. Every example config and skill loads in a test, and `examples/` was checked file by file (4.7).
6a. The book is complete against the coverage matrix, every command and snippet in it was run, `mdbook build` has no warnings, and no legacy page remains (4.5).
6b. Every ADR in `docs/adr` is `Accepted`, true against the code and needed; none is Proposed, Draft, Deprecated or Superseded; the index is exact (4.6).
6c. `README.md` is rewritten per 4.8a: real screenshots, diagrams drawn from the code, accurate lifecycle and safety sections, verified quick start.
6d. `docs/rework`, `docs/design-pack` and `docs/build-plan` no longer exist, nothing references them, and what was still true in them lives in the book, `ARCHITECTURE.md`, an ADR or a module doc (4.9).
7. Every comment or doc that mentions a prompt, a plan or an internal id is gone from committed files.
8. The workspace is no larger in behaviour and measurably smaller in code: the PR states lines and files removed per crate and the test count before and after, with a reason for each test removed.
9. `cargo xtask ci --web`, the web checks including `npm run smoke`, and the WSL Unix suites pass on the branch.

## 8. Tests required

- `baseline_migration_matches_the_expected_schema` (tables, columns, indexes, triggers; fresh database).
- `baseline_migration_is_immutable_after_first_release` (arch-check rule test for ADR 0029).
- `audit_chain_starts_at_the_first_row_of_a_fresh_database` and the existing tamper, truncation and anchor tests, updated.
- `new_database_has_no_legacy_action_surface` or the equivalent compile-level absence, only if section 3.1 leaves any action type in place.
- `example_configs_and_skills_load` (hub, agent, each skill).
- `every_adr_is_accepted_and_indexed` (parses `docs/adr`: status line is `Accepted`, README lists every file; may live in `xtask`).
- A docs check that every public API route, CLI subcommand, table and wire message is named on its reference page, if cheap (4.5 item 6).
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
