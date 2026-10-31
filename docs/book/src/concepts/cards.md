# Verdicts, cards and threads

Every incident gets one card and one thread. Repeats, reminders and the resolution go into the thread, not into new messages.

## The card

A card is short enough to read on a phone, at most eight lines:

```text
**[ACTION NEEDED] nas**
plex is crash-looping after the 1.41 image update
- restarts: 7 in 10 minutes
- last error: migration 42 failed
Next: Pin the previous image tag and restart.
```

- **Badge:** the triage verdict (ACTION NEEDED, MONITOR, EXPECTED, FIXED) or, before triage, the severity.
- **Headline:** one line, at most 120 characters.
- **Up to three bullets:** evidence from triage, or from the latest signal when there is no triage yet.
- **One next step,** when triage suggests one.
- **Buttons:** Ack and Mute 1 h while the incident is active, Details for the full reasoning, and Approve and Deny for each proposed action.

Alerts never wait for triage: a card without triage shows the severity, the incident title and the signal's evidence, and is updated when triage finishes.

Destructive actions never get an approve button. Their card links to the web UI, where they need a typed confirmation. An action without a chat approval token also gets the link instead of buttons.

## The thread

| When | What is said |
| --- | --- |
| The same problem fires again | `Again: plex restarted 7 times in 10 minutes (5th time in 36m)`, at most once every 30 minutes. Repeats in between are counted (`+2 more`). |
| The severity rises | `Escalated to critical: …`, and the card is updated. |
| It is still open | `Still ongoing (1d 2h): …` every 24 hours for warnings, every 4 hours for critical incidents. Settings: `[notify.renotify]`. |
| It resolves | `Resolved after 42m`, and the card shows it as resolved. |

## Quiet hours

During `[notify.quiet_hours]` only critical alerts go out. Everything else is held, and when quiet hours end the held alerts go out as one message:

```text
While you were away: 2 alerts
- [warning] disk.free on /srv below 10%
- [info] backup.service failed (resolved)
```

Alerts that resolved while held are listed as resolved instead of being posted. An alert that escalates to critical during quiet hours goes out at once. Quiet hours use `[hub] timezone`, may span midnight, and follow daylight-saving changes.

## Not notified

Incidents in a maintenance window, or matched by an ack rule, still change and are still stored, but nothing is posted about them.
