# UI 04 · Respond: incidents, approvals and chat

Branch: `ui-respond`. PR: `feat(web): …`. Needs 03. API changes allowed where the pages
need data they cannot get today (say which in the PR).

These are the pages an operator opens when something is wrong. They must answer "what
needs me, how bad, what do I do" in two seconds, on a phone, at night.

## Background (verified on `main`, paths under `web/src/features/`)

- Incidents list: `incidents/IncidentListPage.tsx` (filters, host selector, infinite
  lists), `IncidentGroups.tsx` (groups by state), `IncidentRow.tsx` (a card row: badge
  and title link, then one meta line: host, status, last fired, occurrences, triage
  note), `badge.ts` (moved to `tone.ts` in 01).
- Incident detail: `incidents/IncidentDetailPage.tsx` stacks in one column: a meta line
  (Incidents · host · check target · status), `IncidentCard`, "Check by hand" manual
  steps, `RetryTriage`, `IncidentControls` (Ack, Mute with 1 h/4 h/1 d/1 w, Dismiss
  dialog with reason ≤ 500 chars and "keep quiet for" No/1 d/1 w/30 d/Always, Unmute),
  Details (Markdown), `IncidentChat` (follow-up questions, `AskBox`), proposed actions
  (`approvals/ActionItem`), tools used, and a plain `Timeline` that prints raw kinds
  (`triage.followup`, `incident.opened`, …).
- Approvals: `approvals/ApprovalsPage.tsx` lists proposed actions with
  `ActionItem` (skill, risk as text, status as text, args as `name=value`, reason,
  "Proposed by … · expires …", View incident) and `ApproveControl` (approve with
  password, deny, inline errors).
- Chat: `chat/ChatPage.tsx` (general chat, host picker in `useState` until 03,
  conversation in `sessionStorage`), shared `components/ChatThread.tsx`.

## Steps

1. **Incidents list.**
   - `PageHeader` with a one-line summary in `meta`: "2 need action · 3 to monitor · 1
     pending triage" as toned pills (from the overview endpoint or the list data).
   - `FilterChips` for state groups with counts and tones (Action needed, Monitor,
     Pending, Expected, Acknowledged/Muted, Resolved), plus search by title/check/host
     if the API supports it; if not, add `q=` to the list endpoint (server-side, the
     list is paged). Filters live in the URL query so a filtered view can be shared and
     survives reload.
   - ≥ 900 px: each group as a `DataTable`: status pill, title (the link, keeps its
     accessible name), host, check and target, severity, last fired (`RelativeTime`),
     occurrences, triage note. Row tone stripe; incidents that need action get the
     faint `danger` row tint; resolved rows read quieter (muted text). Below 900 px:
     stacked cards with the same information in the same order.
   - Row quick actions in a `Menu` ("More" `IconButton`): Acknowledge, Mute for…, Open.
     Each reports through `useActionFeedback`; Mute offers Undo (unmute).
   - Empty states per filter ("Nothing needs action right now." with a link to show
     everything) and a real "no incidents yet" first-run state that points to Hosts.
   - Loading uses table/card skeletons; Load more shows progress and its own error.

2. **Incident detail.**
   - Header: `PageHeader` with the title, breadcrumb (Incidents / title), and `meta`:
     status pill, verdict pill, severity pill, host (link), check and target, opened
     and last fired times, occurrences. Primary actions on the right: Acknowledge,
     Mute (menu of durations), Dismiss (dialog), Unmute when muted. Each has a tooltip
     saying what it does and what it does not do ("Stops reminders. The incident stays
     open and still resolves by itself.").
   - Two columns on ≥ 1100 px: left (2fr) card summary, triage report, "Check by hand",
     details, chat; right (1fr, sticky on tall screens) proposed actions, skills and
     tools used, timeline. One column below, with proposed actions directly after the
     summary (the operator's next step comes first on a phone).
   - Triage state as a clear panel: running → `Progress` + "Triage is reading the
     logs…"; skipped/failed → `InlineMessage` with the reason in words (`triage_off`,
     `budget_spent` explained via `describeError`) and Retry with feedback; cached →
     a quiet note "Reused a report from 12 minutes ago". "Check by hand" keeps its
     "Written by the model; Domovoi does not run these." note, styled as a checklist the
     user can tick locally (state in `sessionStorage` per incident, never sent).
   - Dismiss dialog: `Field`s, the consequence spelled out per choice ("Creates a known
     condition: matching incidents are kept in the history and never notify, for
     30 days."), character counter near the limit, submit with `loading`.
   - Timeline: vertical list with a tone dot and an icon per kind and human phrases
     ("Opened", "Triage reported: Action needed", "You asked a follow-up", "Action
     approved by alice"); map known kinds in one table (in `tone.ts` or a sibling
     `timeline.ts`), fall back to the raw kind. Absolute time in a tooltip. Collapse
     long histories with "Show all N".
   - Live: when an SSE event changes this incident, update in place; if the status
     changed while the page is open, a short info toast ("This incident resolved").

3. **Actions and approvals.**
   - One `ActionCard` used on the incident page and the Approvals page: skill name,
     `riskMeta` pill with an `InfoTip` explaining the risk level, `actionStatusMeta`
     pill, host, arguments as a `KeyValueList` in mono with long values wrapped, the
     model's reason, "Suggested by …" line (show `proposed_by` in words today; leave the
     line ready for "Suggested by triage" / "Suggested in chat"), created time, and for
     proposed actions a `Countdown` to expiry that turns `warning` under 10 minutes.
   - Approve/Deny: Approve is primary with the password prompt; Deny is secondary with
     an optional reason. Destructive-risk actions use `ConfirmDialog` with the
     consequence in plain words before the password. After approving, the card shows
     the action moving through `approved → dispatched → succeeded/failed → verified` with
     the `progress` tone and a final toast with the result ("restart-service succeeded
     on nas-01. View output" if output exists). An expired action explains itself
     ("Nobody decided within the hour, so it expired. Triage can propose it again.").
   - Approvals page: `PageHeader` with the waiting count; sections "Waiting for you"
     (proposed, oldest first so the soonest to expire is on top) and "Recently decided"
     (last 24 h, any final status, collapsed by default) if the API can list them (add a
     `since`/status-set filter to `GET /api/v1/actions` if needed). Follows the host
     selector (server filter from 03). Empty: "Nothing to approve. Actions triage
     suggests show up here, and nothing runs until you approve it."

4. **Chat** (general and per incident share `ChatThread`).
   - Message bubbles that read as a console transcript, not a messenger: the user's
     questions right-aligned on a raised surface, answers left on the page surface with
     Markdown, tool calls the model made shown as small collapsible `Tag`s ("read logs
     · nas-01"), proposed actions inline as `ActionCard`s.
   - Composer: autosizing `Textarea`, Enter sends, Shift+Enter newline (say so in the
     hint), disabled with reason while triage is off or the budget is spent, a
     `loading` state with "Thinking…" and a Stop button if the API supports aborting.
   - Errors inline under the failed message with Retry (keep the question text);
     rate limits with countdown.
   - Starter suggestions on an empty general chat (3–4 short questions using the
     selected host's name), plain buttons, no animation.
   - Host: the selected host is the default (03); show which host the chat is about in
     the header.

5. **Catalogue and docs.** Add `ActionCard` (all statuses and risks), the timeline and
   the chat bubbles to `/dev/design`. In `DESIGN.md`, add the "Show an action's result"
   and "Add a status" recipes, with the incident page as the worked example.

6. **Book.** Update screenshots or descriptions in `docs/book/src/concepts/incidents.md`,
   `concepts/actions.md` and `concepts/cards.md` where they describe the UI.

## Tests

- Incidents list: renders a table on a wide viewport (mock `matchMedia` or container
  width) and cards on a narrow one; filter chips update the URL and the list; counts
  show; the title link keeps its accessible name.
- Quick ack from the row: success toast; failure shows the error toast and rolls back.
- Detail: two-column layout on wide screens; triage failed shows the reason and Retry;
  timeline shows human phrases for known kinds and the raw kind for unknown ones.
- Dismiss dialog: consequence text changes with the choice; submit disabled until a
  reason is given (with the reason as tooltip).
- ActionCard: destructive approve asks for confirmation first; expiry countdown turns
  warning; expired action explains itself.
- Approvals: "Waiting for you" is ordered by expiry; follows the host selector.
- Chat: Enter sends, Shift+Enter does not; a failed answer shows Retry with the question
  kept; disabled composer shows its reason.

## Done when

- At 375 px and at 1920 px an operator sees what needs them, how bad it is and the next
  step without scrolling past unrelated content.
- Every action on these pages reports its outcome in words; every status is a toned pill
  with a hint.

## Out of scope

- Who proposed an action as a new audit field (the line is ready; the data comes later).
- Saving the general chat on the hub.
