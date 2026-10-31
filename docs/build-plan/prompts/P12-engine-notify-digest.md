# P12 — Engine II: notification policy, cards, digest, advisories

| | |
| --- | --- |
| Phase | 3, Hub core |
| Depends on | P11 |
| Crates | `domovoi-engine` |
| Size | L |
| Invariants touched | INV-11 (alerts go out even without triage) |

## Goal

Decide what gets said, when and how: card building, the notification policy (first post, folded repeats as one thread line, reminders, resolutions, quiet hours with a held queue), the daily digest with trends and collapsed quiet hosts, advisories, and platform-neutral markdown rendering. Delivery itself is wired in P22.

## Read first

- `ARCHITECTURE.md` §5.7, §13
- Lessons the design encodes: the same lines every morning are noise; two fail2ban waves on one day should be one incident plus one line; long paragraphs are unreadable on a phone.
- P02 `Card`, `Digest`, `Advisory`; P11 `DomainEvent`

## In scope

### Card builder (`cards.rs`, pure)

`fn build_card(incident: &Incident, report: Option<&TriageReport>, actions: &[ActionRequest], host: &HostName, links: &CardLinks) -> Card`:

- Badge: the report verdict if present, else severity.
- Headline: report headline, else the incident title (≤ 120 chars).
- Bullets: up to 3 from report evidence (`label: value`), else up to 3 evidence entries from the latest signal.
- Next step from the report.
- Buttons: `Ack` and `Mute1h` while active; `Details`; per proposed action: `Approve`/`Deny` for `Read`/`Write` risk, `OpenInWeb` for `Destructive` (never an approve button, INV-6). Chat tokens come from the `ActionRequest`.
- `details`: report `summary`, `cause` and `details`, for the thread.

### Notification policy (`notify/`, pure)

`fn plan(event: &DomainEvent, state: &NotifyState, settings: &NotifySettings, now, tz) -> Vec<NotifyAction>` where `NotifyAction` is `PostCard`, `UpdateCard`, `ThreadReply { text }`, `Hold { item }`, `Nothing`. Rules:

- `IncidentOpened` / `IncidentReopened` with `notify` → `PostCard` (open a thread on post). During quiet hours, severity < Critical → `Hold`.
- Card is updated (`UpdateCard`) whenever triage attaches, the verdict changes, an action changes state, or the incident resolves.
- `IncidentFolded` → one `ThreadReply`: "Again: {summary} ({n}th time in {window})". No new card. Rate-limit folded replies to one per 30 min per incident; extra ones increment a counter shown in the next reply.
- `IncidentEscalated` → `ThreadReply` plus `UpdateCard`.
- `IncidentResolved` → `ThreadReply "Resolved after {duration}"` plus `UpdateCard` (resolved badge).
- Reminders: open Warning every 24 h, Critical every 4 h (configurable): `ThreadReply "Still ongoing ({age}): {headline}"`. Not during quiet hours for warnings.
- Quiet hours: the held queue is released as **one** message at the end of quiet hours: "While you were away: {n} alerts" with one line each and links to their threads; incidents resolved while held are listed as resolved.
- Quiet-hour calculations use the configured IANA time zone, including windows that span midnight and DST changes.
- `NotifyState` per incident: message and thread references per gateway, last reminder, last folded reply, held flag. Persisted through `SettingsRepo` or a dedicated table added by a **new** migration file.

### Digest (`digest/`, pure builder + small service)

- `fn build_digest(date, hosts, facts_today, facts_history (14 days), open_incidents, advisories, tz) -> Digest`.
- A host is **quiet** when it has no open incidents, no new advisories and none of its facts changed beyond a small tolerance since the previous digest → one line: "{host}: healthy, nothing changed".
- Otherwise list changed facts with deltas ("Pool free 480 GB, down 41 GB in 7 days"). For numeric facts with a declining trend and a known floor (free space), project time to the floor by least squares over the last 14 days ("full in ~12 weeks at this rate"); only show projections under 180 days.
- Then open incidents (one line each with age and verdict) and active advisories.
- `DigestService::due(now, tz, last_sent) -> bool` (first tick after the configured local time, once per day).

### Advisories (`advisories.rs`)

Rules as pure functions over current state producing `Advisory` upserts and resolutions: `HeartbeatMissing` (no heartbeat configured or last ping failed for > 10 min), `TriageDegraded` (fed by P16), `StaleImage` (image age fact > 180 days), `StackWithoutSource` (fact from the agent when a compose project's config files are missing), `SecretExposed` (config `secrets.exposed` non-empty), `BaselineEntryExpired`, `AgentOutdated` (agent minor version behind hub). Advisories appear in the digest and the web UI only; they never notify by themselves.

### Rendering (`render.rs`, pure)

Platform-neutral markdown for `Card` (header line with badge and host, bullets, next step), thread replies, the held-queue summary and the digest. Discord-specific formatting is P21's job.

## Out of scope

- Sending anything (P22). Discord embeds (P21). Triage (P16).

## Tests required

1. Card builder: insta snapshots for (a) no report, (b) report with Monitor verdict, (c) report with a write proposal (approve/deny buttons), (d) destructive proposal (only `OpenInWeb`), (e) resolved.
2. Policy: `first_open_posts_card_and_thread`, `folded_signal_replies_once_per_30_minutes_with_counter`, `warning_reminder_after_24h_and_critical_after_4h`, `quiet_hours_hold_non_critical_and_post_critical`, `held_queue_released_as_one_message_at_end_of_quiet_hours`, `quiet_hours_spanning_midnight`, `quiet_hours_across_dst_change` (Europe/Sofia, last Sunday of October), `suppressed_events_never_notify`, `resolution_updates_card_and_replies`.
3. Digest: `quiet_host_collapses_to_one_line`, `changed_fact_shows_delta`, `declining_free_space_projects_time_to_full`, `projection_hidden_beyond_180_days`, `digest_lists_open_incidents_and_advisories`, `digest_due_once_per_day_after_local_time`; insta snapshot of a two-host digest rendering.
4. Advisories: one test per rule.
5. `alerts_post_without_triage` (INV-11): an opened incident with no report still yields `PostCard`.

## Architecture gates

- All policy, card, digest and advisory logic is pure and synchronous.
- New tables go in a new migration file (R13).

## Acceptance criteria

- `cargo xtask ci` passes.
- Rendered snapshots are short: card ≤ 8 lines, folded reply 1 line, digest quiet host 1 line.

## Deliverables

- `crates/domovoi-engine/src/{cards,notify/*,digest/*,advisories,render}.rs`
- Possibly `crates/domovoi-store/migrations/0002_notify_state.sql` and its repository (keep store changes minimal and tested)

## PR

Title: `feat(engine): notification policy, cards, digest and advisories`
