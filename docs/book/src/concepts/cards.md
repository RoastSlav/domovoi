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

- **Security events:** triage never calls a `security.*` event EXPECTED: only a person does, with a known condition or by dismissing the incident. Such a verdict becomes ACTION NEEDED. FIXED needs a cited tool result; otherwise it is lowered to MONITOR. In both cases the headline, summary and next step are rewritten to match, and the details keep the model's own text, marked as not accepted.
- **Every security event is triaged on its own evidence.** A cached report is never used for a `security.*` check, an earlier incident is not evidence, and an incident that reopens starts without a verdict. A stored EXPECTED verdict that no person gave is withdrawn when the hub starts.
- **Blind checks:** a card for a check that cannot read the journal says why, and its next step is the fix.
- **Badge:** the triage verdict (ACTION NEEDED, MONITOR, EXPECTED, FIXED) or, before triage, the severity. A resolved card keeps its badge and is marked as resolved; the web UI shows a Resolved badge instead, with the verdict as secondary text.
- **Headline:** one line, at most 120 characters.
- **Up to three bullets:** evidence from triage, or from the latest signal when there is no triage yet.
- **One next step,** when triage suggests one.
- **Buttons:** Ack and Mute 1 h while the incident is active (once acknowledged the card says "Acknowledged" and drops Ack; once muted it says "Muted until HH:MM UTC" and drops both; a press that comes too late is answered in plain words such as "Already muted until 23:35 UTC."), Details for the full reasoning, and Approve and Deny for each proposed step. Each button names the skill and the host, and the card lists the skill, host, risk, every argument, the first characters of the plan hash and when the approval expires. The first six characters of the plan hash travel with the press as a staleness check; they do not replace the dispatch-time check of the full plan.
- **A plan block for each waiting step,** written from the stored step rather than the model's text: the skill, the host, the arguments, the risk, the short plan hash and when the approval window closes. It comes before any model prose, so you read what will run first.

Alerts never wait for triage: a card without triage shows the severity, the incident title and the signal's evidence, and is updated when triage finishes.

Destructive steps, and steps whose risk is unknown, never get an approve button. Their card links to the web UI, where they need a typed confirmation. A step without a chat approval token also gets the link instead of buttons. A step whose arguments are too long to show in the card gets only the link too.

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

Delivery is at least once. The hub records that a card was sent only after the chat platform confirmed it, so a card that could not be delivered is planned again the next time the incident changes, and a reminder or a held alert the next time that minute's tick runs. The same holds for the "while you were away" message: an alert stays held until its card or the summary reached you. A message that a platform accepted but the hub could not record is sent again, so a duplicate is possible after a crash. Discord deduplicates a retried message only if the hub sends a nonce, which it does not, so a duplicate card is possible after a crash.

## Not notified

Incidents in a maintenance window, or matched by an ack rule, still change and are still stored, but nothing is posted about them.
