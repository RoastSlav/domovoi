# How it works

The hub assembles the store, incident engine, agent server, web API, chat adapters and language-model providers into one runtime. Building the runtime opens and migrates SQLite and starts the engine consumers. Running it opens the web and agent listeners and starts chat intake and scheduled jobs.

## From signal to action

```text
agent reports ─┐
signed hooks ──┼─> incident manager ─> SQLite + event bus ─> chat cards
web changes ───┘          │                    │
                          │                    ├─> triage workers ─> report
                          │                    │                      │
                          │                    │                      └─> card update
                          │                    └─> action executor
                          │                                               │
                          └─> incident state, evidence and approvals      │
                                                                           ▼
                                                          approved command on the owning agent
```

The incident manager persists signals and changes before publishing domain events. Matching signals update the same incident according to its confirmation, folding and resolution policy. Triage workers consume confirmed incidents concurrently; a report adds evidence and updates the existing card rather than opening another incident.

Chat delivery plans each change from the notification policy and persisted per-incident state. It creates a card and thread when needed, then updates the same message on later changes. Approvals in chat or the web UI go through the action service. Only an approved action is sent to its owning agent.

## Restart and missed events

SQLite holds incidents, signals, triage reports, action history, chat message references and notification state. A restart reuses those references, so later changes update existing cards instead of posting duplicates.

Every event consumer handles a full or overwritten event buffer by reporting the lag and reloading authoritative state from SQLite. Delivery rebuilds cards for stored incidents, including incidents whose opening event was missed. Approval execution reloads approved actions. Lag therefore causes recovery work, not a crash or a silent loss of state.

## Schedules

All schedules use the runtime clock and the hub's configured timezone:

- once a minute, release notifications whose quiet hours have ended, expire old approvals and send the daily digest when due;
- every five minutes, re-evaluate advisories;
- at 03:30 local time, purge records beyond their retention period;
- at 04:00 local time, back up SQLite;
- at the configured interval, ping the external heartbeat URL.

The digest records its last delivery in SQLite. Restarting the hub does not send the same day's digest again. Heartbeat and delivery failures open advisories; successful later attempts clear them.

## Shutdown

Shutdown stops new intake first: web and agent listeners, chat connections and schedulers. It then gives in-flight triage up to ten seconds to finish before stopping action execution, the incident manager and chat delivery. SQLite closes last. The total budget is 15 seconds; a task that exceeds its phase deadline is cancelled and the runtime reports a shutdown timeout.
