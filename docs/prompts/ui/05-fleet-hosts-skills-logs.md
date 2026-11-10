# UI 05 · Fleet: hosts, host detail, skills and logs

Branch: `ui-fleet`. PR: `feat(web): …`. Needs 04 (it reuses the patterns set there).
API changes allowed where a page needs data it cannot get.

## Background (verified on `main`, paths under `web/src/features/`)

- Hosts list: `hosts/HostsPage.tsx`, already a table (name link without
  `encodeURIComponent`, connection via `HostStatus.tsx`, revoke with inline error,
  `EnrollDialog.tsx` for invites).
- Host detail: `hosts/HostDetailPage.tsx`: `<h1>` host name, then `Tabs` labelled
  "<host> details": Overview (Facts, Recent incidents, Skills sections), Profile
  (`ProfileTab.tsx`, polls until a profile exists), Checks (`ChecksTab.tsx`, overrides
  with save and inline errors; also hosts `DockerPanel.tsx`), Logs (`LogsTab.tsx`), SSH
  logins (`SshLoginsTab.tsx`, 374 lines: split it). Duplicate headings inside tabs.
  `RecentIncidents` shows "No incidents." when its request **failed**.
- Skills: `skills/SkillsPage.tsx` (with "All hosts" handled in 03), `SkillList.tsx`,
  `SkillDetail.tsx` (no loading or error handling).
- Logs: `logs/LogsPage.tsx` (`?source=`, defaulted from the host selector in 03),
  `LogViewer.tsx` always prints "Auto-refresh every 5 s" even when refresh is off.

## Steps

1. **Hosts list.**
   - `PageHeader` with `meta` "5 hosts · 4 online · 1 offline" pills and the primary
     action "Add host" (opens the enroll dialog).
   - `DataTable`: name (link, encoded), connection pill (`hostConnectionMeta`: connected
     / disconnected with "last seen 3 h ago" / never connected), kind tag (agent,
     container agent, external) with a tooltip, agent version with a `warning` tag "Update
     available" when older than the hub (data from the updates endpoint), open
     incidents count (toned, links to the filtered incident list), last seen. Row
     menu: Open, Copy name, Revoke agent (`ConfirmDialog` with consequence, typed name
     for confirmation, toast on result). Leave a column/tag slot for maintenance.
   - Offline hosts sort first by default; sortable columns with `aria-sort`.
   - Empty: first-run state explaining hosts and agents with "Add your first host".
   - Enroll dialog: steps shown as a short numbered list (create invite → copy the
     command → wait for the agent), the command in a `CopyBlock`, an invite expiry
     `Countdown`, and a live "Waiting for the agent to connect…" that turns into a
     success state ("nas-01 connected") when the hosts query sees the new host.

2. **Host detail.**
   - Header on one line where it fits: name, connection pill, kind tag, version (with
     update tag), open incidents count (link), last seen; actions on the right in a
     `Menu` (Copy name, Revoke) plus a slot for a future "Maintenance" button.
   - Tabs with counts where useful (Checks with failing count in `danger`, Logs, SSH
     logins). Remove duplicate headings inside tabs; each tab panel has one heading
     level below the page title. The tab is reflected in the URL (`?tab=checks`).
   - Overview: facts as a `KeyValueList` in a `Grid` of `Card`s (system, disk, memory,
     network: group by what the facts contain), recent incidents as a compact table with
     pills, skills as a compact list with risk pills. `RecentIncidents` uses
     `QueryBoundary`: a failure shows `LoadError`, never "No incidents.".
   - Profile: what the profile is (`InfoTip`), its age, sections as collapsible cards;
     a failed tool output is a `warning` `InlineMessage` "Could not read <tool>: …",
     not a gap. While no profile exists: "Building the baseline. This takes a few
     minutes after the agent connects." with `Progress`.
   - Checks: a table of checks with status pill (ok / failing / disabled), threshold
     and override values, source tag (agent default / hub override), edit in a row
     drawer or dialog with `Field`s and hints, `SubmitBar` with unsaved count, per-field
     issues from the hub, success toast. Docker panel in the same style.
   - SSH logins: split `SshLoginsTab.tsx` under the line limit (data, table, filters);
     a table with user, source address, time, result pill (accepted `neutral`, failed
     `warning`, many failures from one address `danger` with a hint), filters as chips.
   - Logs tab: the shared `LogViewer` (step 4).

3. **Skills.**
   - List: `DataTable` or cards with name, description, risk pill with `InfoTip`
     (what read/write/destructive may do and who approves), timeout, applies-to tags,
     last used. Leave a header action slot for "Add skill" and a row slot for
     "Edit/Delete" and a "hub-managed" tag.
   - Detail (`SkillDetail.tsx` via `QueryBoundary`): parameters as a table (name, type,
     constraints, description), README as Markdown, script in a read-only mono block
     with copy.

4. **Logs.**
   - `LogViewer` takes `refreshMs: number | null` and prints the auto-refresh line only
     when set; a "Live" `Switch` controls it and pauses while the tab is hidden.
   - Monospace lines with a level column toned subtly (error `danger` text, warn
     `warning`, info default, debug `text-subtle`); never tint whole lines. Search box
     filters and highlights matches; "Jump to latest" button appears when the user
     scrolled up; wrap toggle; copy visible lines.
   - Long logs render without jank (window the list or cap the rendered lines with
     "Show older"; measure with 10k lines).
   - Source picker follows the host selector (03) and shows each source's host.

5. **Catalogue and docs.** Add the host header, facts cards, checks row editor and log
   viewer to `/dev/design`; add "Add a table column" and "Add a tab" recipes to
   `DESIGN.md`. Update `docs/book/src/guide/agents.md` (enrolling from the UI) and
   `guide/skills.md` where they describe the pages.

## Tests

- Hosts: offline hosts sort first; revoke asks for the typed name and reports the
  result; the host link is encoded (`nas 01` → `/hosts/nas%2001`).
- Enroll: the dialog shows the waiting state, then success when a new host appears.
- Host detail: tab follows `?tab=`; failing checks count shows; `RecentIncidents` error
  shows Retry; profile tool failure shows a warning message.
- Checks: an override saves with a toast; a hub issue marks the field.
- Skills: All hosts behaviour (from 03) still holds; detail loading and error states.
- LogViewer: no auto-refresh line when `refreshMs` is null; search filters lines;
  10k lines render (smoke test with a time budget is not needed; assert a cap or
  windowing).

## Done when

- Every Fleet page uses the kit, follows the host selector, has all four states, and
  reads well at 375–1920 px in both themes.
- `SshLoginsTab.tsx` is under the line limit and removed from the lint override.

## Out of scope

- Maintenance windows, adding/editing skills, agent self-update (slots only).
