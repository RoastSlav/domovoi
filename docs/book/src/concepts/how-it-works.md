# How it works

The hub assembles the store, incident engine, agent server, web API, chat adapters and language-model providers into one runtime. Building the runtime opens and migrates SQLite and starts the engine consumers. Running it opens the web and agent listeners and starts chat intake and scheduled jobs.

## From signal to approved step

```text
agent reports ─┐
signed hooks ──┼─> incident manager ─> SQLite + event bus ─> chat cards
web changes ───┘          │                    │
                          │                    ├─> investigation (operation) ─> report
                          │                    │                      │
                          │                    │                      └─> card update
                          │                    └─> operation service
                          │                                               │
                          └─> incident state and evidence                 │
                                                                           ▼
                                                  approved step on the owning agent
```

The incident manager persists signals and changes before publishing domain events. Matching signals update the same incident according to its confirmation, folding and resolution policy. Each confirmed incident is investigated by an [operation](operations.md); its report adds evidence and updates the existing card rather than opening another incident.

Chat delivery plans each change from the notification policy and persisted per-incident state. It creates a card and thread when needed, then updates the same message on later changes. Approvals in chat or the web UI go through the same use-case layer. Only an approved step is sent to its owning agent.

## Operator

The Operator answers general questions about the operator's servers. Its conversations are durable: they are stored on the hub, survive a restart, and can be picked up from any browser. Each conversation has an explicit scope, all hosts or one host, chosen when it is created. The scope never follows the host selector at the top of the page. Answers stream in live while they are written. **Stop** cancels an answer that is still being written, and **Retry** asks the last question again after its answer failed or was stopped.

The Operator is read-only. It cannot propose, approve or run a step. Everything sent is redacted and size-bounded, and the monthly model budget applies. Conversations are kept for 180 days by default. If the hub restarts while an answer is being written, that answer becomes "not answered"; it is not resumed on its own. See [Operator](operator.md) for the details.

## Restart and missed events

SQLite holds incidents, signals, triage reports, operations and their steps, chat message references and notification state. A restart reuses those references, so later changes update existing cards instead of posting duplicates.

Every event consumer handles a full or overwritten event buffer by reporting the lag and reloading authoritative state from SQLite. Delivery rebuilds cards for stored incidents, including incidents whose opening event was missed. The operation service resumes approved steps from the store. Lag therefore causes recovery work, not a crash or a silent loss of state. The browser stream resumes from the last event it saw, and the hub tells it to refetch when it cannot replay what the browser missed.

## Schedules

All schedules use the runtime clock and the hub's configured timezone:

- once a minute, release notifications whose quiet hours have ended, expire approvals whose window has closed and send the daily digest when due;
- every five minutes, re-evaluate advisories;
- at 03:30 local time, purge records beyond their retention period (see [Retention](../guide/retention.md));
- at 04:00 local time, back up SQLite;
- at the configured interval, ping the external heartbeat URL.

The digest records its last delivery in SQLite. Restarting the hub does not send the same day's digest again. Heartbeat and delivery failures open advisories; successful later attempts clear them.

## Shutdown

Shutdown stops new intake first: web and agent listeners, chat connections and schedulers. It then gives in-flight triage up to ten seconds to finish before stopping the operation service, the incident manager and chat delivery. SQLite closes last. The total budget is 15 seconds; a task that exceeds its phase deadline is cancelled and the runtime reports a shutdown timeout.
