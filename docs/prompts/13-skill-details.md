# 13 · Feature: more details about each skill in the web UI

## Goal

Clicking a skill on the Skills page (or a host's Skills tab) opens a detail view with:
description, risk explained, parameters with their exact limits, which checks it applies
to, timeout, how it is verified, its manifest hash (short), its README from the skill
folder, and its recent runs on that host.

## What is already known (verified)

- The agent reports `SkillSummary` (`crates/domovoi-core/src/skill/manifest.rs`): name,
  description, risk, params, applies_to, `manifest_hash`, timeout, `verify`. By design it
  never reports the entry path, `run_as` or the output limit; keep it that way.
- `GET /api/v1/hosts/{name}/skills` (`crates/domovoi-hub/src/api/hosts.rs`, `skills()`)
  maps it to `SkillDto` (`api/dto_admin.rs`) **without** `manifest_hash` and `verify`.
- Starter skills ship a `README.md` in their folder (`examples/skills/*/README.md`), but
  the agent never reads it. The catalogue is `crates/domovoi-runner/src/catalog/mod.rs`
  (`LoadedSkill`, `SkillCatalog::summaries()`); loading lives in `catalog/checks.rs`.
- Actions have the skill name; `GET /api/v1/actions` lists them (status filter only).
- UI: `web/src/features/skills/SkillList.tsx` renders `SkillCard`s;
  `web/src/components/Dialog.tsx` exists.

## Read these files

- `crates/domovoi-core/src/skill/manifest.rs` (`SkillSummary`, `summary()`)
- `crates/domovoi-runner/src/catalog/mod.rs` and `catalog/checks.rs`
- `crates/domovoi-hub/src/api/hosts.rs`, `api/dto_admin.rs`
- `web/src/features/skills/SkillList.tsx`, `web/src/components/Dialog.tsx`

## Steps

1. **Core.** Add to `SkillSummary`
   `/// The skill folder's README.md, at most 8 KiB; absent when there is none.`
   `#[serde(default, skip_serializing_if = "Option::is_none")] pub readme: Option<String>,`
   `SkillManifest::summary()` sets `readme: None`. Fix every `SkillSummary { .. }`
   literal in the workspace (search for `SkillSummary {`).
2. **Runner.** Give `LoadedSkill` a `readme: Option<String>` field, filled where skills
   are loaded in `catalog/checks.rs`: read `<dir>/README.md` if it is a regular file, cut
   to 8192 bytes on a character boundary, ignore errors. In `summaries()`, set
   `summary.readme = skill.readme.clone()`.
3. **Protocol.** `Hello.skills` carries `SkillSummary`, so the new optional field is
   additive. Update the hello snapshot (`INSTA_UPDATE=always cargo test -p domovoi-proto`)
   and bump the protocol minor as in rule 10 of `00-rules.md`.
4. **API.** Add to `SkillDto`: `manifest_hash: String` (hex),
   `verify: Option<VerifyDto { recheck: bool, delay_secs: i64 }>`, `readme: Option<String>`.
   Map them in `hosts::skills()`. Regenerate OpenAPI.
5. **Web.** In `SkillList.tsx`, make the skill name a button that opens a `Dialog`
   (new file `web/src/features/skills/SkillDetail.tsx`) showing:
   - risk with an explanation (read: "Only looks. Triage may run it on its own.";
     write: "Changes something. Runs only after you approve it.";
     destructive: "Can lose data. Needs the skill name typed to approve; never from chat.");
   - parameters table: name, type and limits (reuse `kindText`), required, description;
   - applies to (or "any check"), timeout, verification ("re-runs the check after 30 s"
     or "none"), manifest hash (first 12 hex chars, full value in `title`);
   - README rendered with the existing `Markdown` component;
   - recent runs: `useActions()` (from `web/src/api/hooks/actions.ts`, no status
     argument) filtered client-side to this skill and the host's id; show the last 10:
     status, when, link to the incident.

## Tests

- Runner: a skill folder with a README reports it in `summaries()`; one without reports
  `None`; a 20 KiB README is cut to 8 KiB.
- Hub: the skills endpoint returns `manifest_hash` and `readme`.
- Web: clicking a skill opens the dialog with parameters and README.

## Done when

- The detail dialog shows everything listed in the goal; old agents (no README) still
  work and show "No README".
- Snapshots, OpenAPI and all tests are updated and pass.
