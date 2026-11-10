# UI 06 · Knowledge and system: known conditions, notes, digest, settings, setup, sign-in

Branch: `ui-system`. PR: `feat(web): …`. Needs 05. API changes allowed where a page
needs them (for example richer issue paths for settings).

## Background (verified on `main`, paths under `web/src/features/`)

- Known conditions: `rules/RulesPage.tsx`, `rules/AddRuleForm.tsx` (248 lines;
  predicates with ops `equals | contains | at_most | at_least | in_network`; shows an
  empty host list on error).
- Notes: `notes/NotesPage.tsx` (search, host selector), `NoteEditorPage.tsx`
  (Markdown), `ImportDialog.tsx`, `HostScope.tsx` (empty list on error).
- Digest: `digest/DigestPage.tsx` (preview, send to Discord, errors inline, "Could not
  build the digest" without retry).
- Settings: `settings/SettingsPage.tsx`, `EditableSettings.tsx`, `SectionForm.tsx`,
  `fields.tsx` (269 lines), `form.ts` (265), `sections.ts` (321), `useApply.ts`
  (apply and restart), `TestIssues.tsx`, `DiscordSetupHelp.tsx`. Sections: Hub,
  Notifications, Incidents, Discord, Models and triage, Fallback model, Webhooks,
  Exposed secrets, Probes, Heartbeat, Backups, Updates. Each field shows where its value
  comes from; secrets show set/unset.
- First run: `setup/FirstRun.tsx` (403 lines; a step wizard with connection tests; any
  error, even a network error, reads "Settings are managed in hub.toml"),
  `SetupBanner.tsx`.
- Sign-in: `auth/LoginPage.tsx`, `auth/SetupPage.tsx` (account creation).
- Updates: `updates/UpdateBanner.tsx`, `UpdateAbout.tsx`, `HowToUpdateDialog.tsx`.

## Steps

1. **Known conditions.**
   - `PageHeader` with an `InfoTip` explaining what a known condition is ("Incidents
     that match are kept in the history and never notify.").
   - Table: what it matches (predicates rendered as readable phrases, "disk usage on
     /srv at most 95 %"), host, created by and when, expires (`Countdown`/relative,
     `neutral` "Always" tag), matches so far if the API can count them. Row menu: Delete
     (confirm, toast with Undo if recreating is possible, otherwise no Undo).
   - `AddRuleForm` as a dialog or side panel built from `Field`s: predicate rows with
     add/remove, op as `Select` with plain-language labels, value inputs that match
     the op (CIDR hint for `in_network`), live preview sentence of the rule, issues from
     the hub per row. Split the file under the line limit. Host list load error shows
     Retry (via `QueryBoundary`/inline).
   - Leave a section or tab slot for maintenance windows.

2. **Notes.**
   - List: cards or table with title, host scope tags, updated time, a two-line excerpt;
     search with highlighted matches, empty search result state with "Clear search".
   - Editor: two-pane edit/preview on wide screens, tabs on phones; unsaved indicator
     and leave-page guard (React Router blocker); Ctrl/Cmd+S saves (shown in a tooltip
     on Save with `Kbd`); save and delete through `useActionFeedback`; delete confirms.
     `HostScope` error shows Retry.
   - Import dialog: drop zone plus file input, a result summary as toned counts
     (imported `success`, skipped `neutral`, failed `danger` with reasons).

3. **Digest.**
   - Preview rendered as it will look (sections per host with toned facts), the next
     scheduled send time, and "Send now" with `loading`, success toast ("Digest sent to
     Discord") and `describeError` failures (`delivery_failed` explained). Build errors
     use `LoadError` with Retry. Leave room for a cost line.

4. **Settings.**
   - Layout: section nav on the left (sticky, with a `warning` dot on sections with
     unsaved changes and a `danger` dot on sections with issues), the section form on
     the right; on phones, the section list is a page and each section opens as its own
     view with a back link. Section in the URL (`/settings/discord`).
   - Fields: `Field` with label, hint, the value's source as a `Tag` ("from hub.toml",
     "from environment", "set here", "default") with a tooltip explaining precedence;
     fields locked by file or environment are read-only with a `disabledReason`
     ("Set in hub.toml; edit the file to change it."). Secrets: set/unset `Tag`, a
     "Replace" action, never shown.
   - Saving: `SubmitBar` with the unsaved count, Discard (confirm if many), Save. Hub
     issues map to fields (`FormErrorSummary`); if issue paths cannot be mapped reliably,
     improve the paths in the hub. "Restart required" changes say so before saving and
     after saving show a `Banner` "Restart to apply 2 changes" with Restart; the restart
     shows progress ("Restarting the hub…", then "Back online" when the session query
     succeeds again) and errors in words.
   - Connection tests (Discord, models, webhooks, heartbeat): a Test button per section
     with `loading`, result as an `InlineMessage` (`success` with what was checked,
     `danger` with the reason and the fix), `TestIssues` restyled.
   - Split `fields.tsx`, `form.ts` and `sections.ts` under the line limit (field
     renderers per type; section definitions per group).
   - Leave clean section slots for **Account** (change password, sign out other
     sessions) and for a "Last backup" status line in **Backups**.
   - "About": hub version, agent versions summary, update status (`UpdateAbout`).

5. **First run and sign-in.**
   - Login and account setup: a centred card on the paper background with the brand
     mark, `Field`s, password visibility toggle (`IconButton` with label),
     caps-lock hint, `loading` submit, `bad_credentials` and rate limit (countdown)
     messages in words, no layout jump when an error appears. Setup shows the password
     rule as a live checklist.
   - First-run wizard: a stepper with step names and states (done `success`, current
     `progress`, skipped `neutral`), each step a `Section` with its fields and a Test
     button, Skip with a tooltip saying what skipping means, Back/Next, final summary of
     what was configured and what was skipped with links to Settings. Distinguish errors:
     a network failure says the hub is unreachable (Retry); only a real "managed by
     file" response says "Settings are managed in hub.toml". Split `FirstRun.tsx` under
     the line limit.
   - `SetupBanner`: `Banner` with what is left to configure and a link.

6. **Updates.** `UpdateBanner` as an `info` `Banner` with the version, a "What's new"
   link and "How to update" (dialog with `CopyBlock`s per install method), dismiss per
   version.

7. **Catalogue and docs.** Add the settings field states (sources, locked, secret,
   issue), the stepper and the auth card to `/dev/design`; add the "Add a setting" recipe
   to `DESIGN.md`. Update `docs/book/src/` pages that describe Settings, first run and
   sign-in (find them with `grep -ril "settings page\|first run" docs/book/src`).

## Tests

- Known conditions: the preview sentence matches the predicates; delete confirms and
  toasts; host list error shows Retry.
- Notes: leaving with unsaved changes asks first; Ctrl+S saves; import summary counts.
- Digest: send success toast; `delivery_failed` message in words; preview error Retry.
- Settings: unsaved dot on the section; a hub issue focuses the field from the summary;
  a locked field is read-only with its reason; restart flow shows progress then success
  (mock the session query); section follows the URL.
- First run: a network error says the hub is unreachable, not "managed in hub.toml";
  skipping a step marks it skipped in the summary.
- Login: bad credentials message; rate limit countdown.
- The split files keep their tests passing.

## Done when

- Every page in the app uses the kit; no page still has the old look.
- `FirstRun.tsx`, `fields.tsx`, `form.ts`, `sections.ts` and `AddRuleForm.tsx` are under
  the line limit and the lint override list is empty.

## Out of scope

- The Account and Backups features themselves, maintenance windows (slots only).
