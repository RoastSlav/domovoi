# 01 · Fix: the Discord test fails in the setup wizard but works in Settings

## Goal

Pressing **Test** on the Discord step of the first-run wizard (`/welcome`) must post the
test message whenever the token and channel are right, and must show *why* when it fails.

## What is already known (verified, do not re-investigate)

1. `web/src/features/setup/FirstRun.tsx`, `testStep()` sends `changes(skipped, step)`:
   only the fields of the current step. The `changes()` helper also adds
   `discord.enabled = true` as soon as a bot token is typed.
2. The hub validates the whole candidate config before testing
   (`crates/domovoi-hub/src/settings/connection.rs`, `candidate()` →
   `load_hub_layered`). `crates/domovoi-config/src/validate.rs` rejects an enabled
   `[discord]` with an empty `allowed_user_ids`. So pressing Test before filling
   "Users who may approve" fails with HTTP 400 "the settings are not valid".
3. The wizard shows only `tester.error.message` and drops `tester.error.issues`, so the
   user never sees which field is wrong. `SectionForm.tsx` has the same gap.
4. The Discord test card links to the hub's public URL. In the wizard the Hub step's
   `web.public_url` is not part of the candidate, so links fall back to the listener
   address.
5. After saving, every field is stored, which is why the test in Settings works.

## Read these files

- `crates/domovoi-hub/src/settings/connection.rs`
- `web/src/features/setup/FirstRun.tsx`
- `web/src/features/settings/SectionForm.tsx`
- `web/src/features/setup/firstRun.test.tsx`

## Steps

1. **Hub: a Discord test does not need approvers.** In `connection.rs`, in
   `SettingsService::test`, when `target == Target::Discord`, test with a copy of `patch`
   in which `discord.allowed_user_ids` is `["1"]` if the patch has no non-empty list
   there (missing, `null` or `[]`). Write a small private helper
   `fn with_test_approver(patch: &Map<String, Value>) -> Map<String, Value>` next to
   `scrub`, in the same spirit as `models()`, which inserts a placeholder model. Leave
   other targets unchanged.
2. **Web: test Discord with the Hub step's values too.** In `FirstRun.tsx`:
   - In `changes()`, only add `discord.enabled` when step 1 is among the kept steps:
     replace `!without.has(1)` with
     `(only === undefined ? !without.has(1) : only === 1)`.
   - Change `testStep` so the Discord test merges the Hub step:
     ```ts
     const testStep = () => {
       const current = changes(skipped, step);
       const isDiscord = STEPS[step]?.test === "discord";
       tester.mutate(isDiscord ? { ...changes(skipped, 0), ...current } : current);
     };
     ```
     The Hub step and the Discord step never share a top-level key, so a shallow spread
     is enough.
3. **Web: show the issues of a failed test.** In both `FirstRun.tsx` and
   `SectionForm.tsx`, below the existing `tester.error` message, render each
   `tester.error.issues` entry (when `tester.error instanceof ApiFailure`) as
   `<p className={styles.status}>{path ? `${path}: ` : ""}{message}</p>`, the same way the
   save failure renders its issues.

## Tests

- `web/src/features/setup/firstRun.test.tsx`:
  - Keep `first_run_test_sends_only_the_fields_of_the_step_being_tested` passing (the
    model test must still not send `discord`).
  - Add `first_run_discord_test_includes_the_public_url`: fill the Discord step, press
    Test, assert the request body has `changes.web.public_url` and `changes.discord`.
  - Add `first_run_test_failure_lists_each_issue`: route
    `POST /api/v1/settings/test/discord` to
    `failure(400, "invalid", "the settings are not valid", undefined, [{ path: "discord.channel_id", message: "bad id", severity: "error" }])`
    and assert "discord.channel_id: bad id" is shown.
- Rust: add a unit test module at the bottom of `connection.rs` that checks
  `with_test_approver` fills `["1"]` for a missing, `null` and empty list and keeps an
  existing non-empty list unchanged.

## Done when

- The Discord test in the wizard succeeds with only token and channel filled in.
- Every failed test (wizard and Settings) lists the hub's issues by field.
- `cargo test -p domovoi-hub` and `npm test` pass.

## Do not

- Do not relax `validate.rs`: saving Discord without approvers must still fail.
- Do not send the Discord fields with the model or heartbeat tests.
