# Logs in the web UI

The Logs page shows what the hub itself was doing: checks that ran, incidents that fired, triage runs, enrolments, the errors it hit. It answers "what happened this morning" without leaving the browser.

## What a line looks like

Lines come out oldest first, so the newest one is at the bottom:

```text
12:04:11 INFO domovoi_hub.checks: disk.usage on /srv is 91% full
12:05:02 ERROR domovoi_hub.chat: gateway rejected key=[REDACTED]
```

The time is the hub's clock, printed in your browser's zone. The name before the colon is what logged the line, and after the colon comes the message with its fields. The page prints one line at a time in red for `error` and amber for `warn`.

The page only reaches back as far as the hub kept in memory: the newest 2000 lines, since it started. Longer history goes to `journalctl` when you run the hub under systemd, or to whatever you redirected its standard error into. Lines whose message is longer than 2 KiB are cut.

## Choosing what to read

- **Level and above.** `error`, `warn`, `info` (the default) or `debug`. What exists at all depends on how the hub was started: `--log-level`, or `RUST_LOG`, decides what it logs in the first place. A hub started at `warn` logged nothing at `info`, and the page has nothing to show for that level.
- **Lines.** How many to ask for, 100 to 1000 in the page; the hub answers up to 2000.
- **Source.** The hub, or one of the hosts with an agent. The hub's recent lines are captured in memory; an agent's lines stay on its own machine, but the hub asks the agent for the newest ones through its link, so picking a host shows what that agent was doing there. The lines come back scrubbed again on the hub's side, so an agent that logged a secret without scrubbing it is still safe. A window of lines that does not fit one link frame is asked for in parts and joined back on the hub. An agent from a release that cannot send logs answers "the agent is too old to send logs; update it", and the page shows that.
- **Auto-refresh.** Every 5 seconds, or only when the person clicks. The Refresh button asks right away either way.

The page asks again by itself every 5 seconds while Auto-refresh is on, and the Refresh button asks right away in all cases.

## Secrets

A line never carries a configured secret: those values are held as secret types whose printed form is never the value. And a line is scrubbed while it comes in, with the rules the hub had at that moment, so a secret is replaced with `[REDACTED]` before the hub ever stores the line. Rotating a secret through the settings page cannot uncover a line that was already captured: the earlier line keeps the scrubbing it was captured with. Lines the hub sends to the page go through the same redactor that scrubs chat cards, which changes nothing in an already scrubbed line.
