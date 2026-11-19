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

- **Badge:** the triage verdict (ACTION NEEDED, MONITOR, EXPECTED, FIXED) or, before triage, the severity. A resolved card keeps its badge and is marked as resolved; the web UI shows a Resolved badge instead, with the verdict as secondary text.
- **Headline:** one line, at most 120 characters.
- **Up to three bullets:** evidence from triage, or from the latest signal when there is no triage yet.
- **One next step,** when triage suggests one.
- **Buttons:** Ack and Mute 1 h while the incident is active, Details for the full reasoning, and Approve and Deny for each proposed action, including the actions an investigation proposed as steps. Each button names the skill and the host, and the card lists the skill, host, risk, every argument, the first characters of the plan hash and when the approval expires. The first six characters of the plan hash travel with the press as a staleness check; they do not replace the dispatch-time check of the full plan.
- **A plan block for each waiting action,** written from the stored action rather than the model's text: the skill, the host, the arguments, the risk, the short plan hash and when the approval window closes. It comes before any model prose, so you read what will run first.

Alerts never wait for triage: a card without triage shows the severity, the incident title and the signal's evidence, and is updated when triage finishes.

Destructive actions, and actions whose risk is unknown, never get an approve button. Their card links to the web UI, where they need a typed confirmation. An action without a chat approval token also gets the link instead of buttons. An action whose arguments are too long to show in the card gets only the link too.

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

Alerts still open also get their cards, so they have buttons and reminders. Alerts that resolved while held are listed as resolved instead of being posted, unless that message cannot be delivered, in which case they get cards too. An alert that escalates to critical during quiet hours goes out at once. Quiet hours use `[hub] timezone`, may span midnight, and follow daylight-saving changes.

## Delivery guarantees

Delivery is at least once. The hub records that a card was sent only after the chat platform confirmed it, so a card that could not be delivered is planned again the next time the incident changes, and a reminder or a held alert the next time that minute's tick runs. The same holds for the "while you were away" message: an alert stays held until its card or the summary reached you. A message that a platform accepted but the hub could not record is sent again, so a duplicate is possible after a crash. Discord deduplicates a retried message only if the hub sends a nonce; it does not yet, so a duplicate card is possible after a crash.

## Not notified

Incidents in a maintenance window, or matched by an ack rule, still change and are still stored, but nothing is posted about them.
