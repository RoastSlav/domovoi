# Signals and incidents

Checks, probes, agents and webhooks send **signals**: "this is firing" or "this is fine again", about one check on one host, optionally for one target such as a container or a mount. Domovoi turns signals into **incidents**, the problems you actually hear about.

## Fingerprints

Each signal has a fingerprint: a hash of its host, check and target. Counts, ids, timestamps and measurements are never part of it; they travel as evidence. So the 50th "plex restarted" signal has the same fingerprint as the first, and folds into the same incident.

## From signal to incident

One component, the incident manager, makes every change to incidents, one at a time. It stores each change before anyone is told about it, so a crash never leaves a notification for something that was not stored.

A signal goes to the first of these that applies:

1. **An active incident with the same fingerprint.** A firing signal is folded in: the incident counts it, keeps the highest severity seen, and records the signal's `value` evidence in its history. A resolved signal resolves it.
2. **The same fingerprint resolved within the reopen window** (6 hours by default). A firing signal reopens that incident instead of starting a new one.
3. **An active incident about the same thing from another source.** If an Uptime Kuma monitor for `https://plex.example:32400` goes down and, within 5 minutes, the agent reports container `media_plex_1` unhealthy on the same host, both are about `plex`, so they become one incident.
4. **Nothing.** A resolved signal with nothing to resolve is stored and otherwise ignored.
5. **A new incident.**

To decide that two targets are about the same thing, Domovoi lowercases them and strips URL schemes, credentials, ports, paths and domains, replica suffixes (`plex-1`, `plex_2`) and compose project prefixes (`media_plex_1`). `plex`, `plex-1`, `media_plex_1` and `https://plex.example:32400` are all `plex`.

A grouped incident resolves only when every source in it has recovered. If the web check comes back while the container is still unhealthy, the incident stays open.

## Confirmation and flaps

A new incident starts **pending**. It opens after 2 consecutive firing signals, or at once for signals marked immediate, such as running on battery. A pending incident that resolves before it is confirmed is a **flap**: it is deleted, counted in the `domovoi_incident_flaps_total` metric, and nobody is told. The signals themselves are kept.

## Quiet incidents and mutes

Every 30 seconds the manager checks the time. A muted incident whose mute has ended opens again. With a TTL configured, an incident that has not fired for that long resolves.

## When you are not told

The incident still changes and is still stored, but its events are marked as not to be notified:

- **Maintenance windows:** while a window covers the host.
- **Ack rules:** while an ack rule matches the signal by host, check, target pattern and evidence, for example "`disk.free` on `/backup`, where filling up is expected".

## Known conditions

A known condition says "this firing is expected, so leave it alone". Add one under **Known conditions** in the web UI: the check key, the host it applies to (any host, or one named host), a target pattern (`*` allowed at the ends), why it is acceptable (the note), conditions on the evidence, and how long it lives — 1 hour, 1 day, 1 week, 30 days, or never. As you fill the form in, a sentence under it says in words what the rule will match, and the list shows each rule the same way. Each condition compares one evidence key with a value: equals, contains, is at most, is at least,
or is in network, whose value is one address, such as `192.168.1.20`, or a range in CIDR form,
such as `192.168.1.0/24`. All of them must hold for the rule to match.

Signals that match a known condition are stored, incidents still change, and the history stays complete; nothing is notified and triage does not run for them. The page lists every rule, soonest expiry first, rules that never expire last, and shows expired rules so you can delete them. A rule stops applying on its own when it expires.

A new rule applies to signals that arrive after it is created. An incident that is already open keeps its card and its thread: a rule only holds back the notifications the next signal would cause.

A value that looks like a number in an **equals** condition is compared as a number. `port equals 22` matches a check reporting the number 22; `user equals 1000` does not match evidence stored as the text `1000`. Built-in checks report numbers as numbers, so this is mostly about webhook evidence, where a number arriving as a string needs **contains** rather than **equals**.

You can dismiss an incident instead of writing a rule by hand: on the incident, choose **Dismiss**, say why this is not a real problem, and pick how long the check should stay quiet — 1 day, 1 week, 30 days, always, or **No** to close the incident without a rule, so the check alerts again next time it fires. Dismissal closes the incident right away with the verdict Expected, and the rule it creates matches that incident's host, check and target exactly, so only this one condition goes quiet. Your reason becomes the rule's note, so the **Known conditions** page can say later what it covers.

A dismissed incident that keeps firing reopens within the reopen window, quietly: no alert, no triage, and no notification. It keeps the Expected verdict, so it sits under **Expected** — nothing is asked of anyone about it. After the reopen window passes, the condition starts a **new** incident: the rule keeps it silent, it has no verdict, and it shows under **Action needed**. A nightly condition that is dismissed as always-quiet shows a new incident there every night, which is what "quiet" means: not told about it, not counted as something you have triaged.

## What triggers triage

An incident asks for triage when it opens, when it reopens and when its severity rises.

Open an incident and look at its **Triage** section. With a report, **Run triage again** runs the model from scratch with the current skills, notes and evidence, even when a cached report would fit. When the last run was skipped or failed, the section says why in words and the button says **Retry triage**; before any run it says **Run triage**. While a run you started is going, the section shows progress in place of the result. Every run counts against the same hourly and monthly caps as any other.

A run deferred by the hourly cap may re-run on its own once a run slot frees, as long as the incident is still open, unmuted, and not already triaged by a newer request. A failed run retries after an hour; retries for one incident stop after three failed runs, and once the triage-degraded advisory is up, newly failed runs are not deferred at all. Muted incidents are never retried; acknowledged ones still are. Pending retries live in memory, so they are dropped when the hub restarts.

## In the web UI

The **Incidents** page opens with one line that says what needs you ("2 need action", "3 to monitor", "1 pending"), then sorts every incident into one group by what it asks of you, the most urgent first:

- **Action needed** — open incidents nobody has acknowledged, unless triage called them expected or only worth watching. An open incident triage has not looked at is here too, and so are the rule-silenced incidents a known condition makes, silent and untriaged.
- **Monitor** — open incidents triage said to keep an eye on.
- **Pending** — not confirmed yet. Domovoi alerts if the check keeps failing.
- **Expected** — open, but intended: dismissed as a known condition, or judged expected by triage. Dismissed ones stay quiet while their rule holds.
- **Acknowledged or muted** — someone has seen it or silenced it for a while. Muted ones return when the mute ends.
- **Resolved** — problems that cleared. The group starts closed.

The chips above the list show one group at a time, with how many incidents each holds; **All** shows every group. The search box matches the title, the check and its target, and the severity filter narrows any view. The group, the search and the severity are in the page's address, so a filtered view survives a reload and can be shared, and the host chosen in the top bar applies to all of it. The hub does the filtering, so a group is complete however long the history is; **Load more** fetches the next page.

On a wide screen each group is a table: where the incident stands, its title, host, check and target, severity, when it last fired, how many times, and what triage said. Incidents that need action carry a faint tint; resolved ones read quieter. On a phone the same facts stack into cards. The **More** button on a row acknowledges, mutes for a while (with **Undo**), or opens the incident, and says in a short message how it went.

Empty groups are hidden. A filter with nothing in it says so and links back to everything; a hub with no incidents at all points you to **Hosts**. If a request fails with nothing on screen, the page says the incidents could not be loaded and offers **Retry**, so a hub that cannot be read never looks like a quiet night. A refresh that fails over rows already shown keeps them and notes "Could not refresh."

An incident's own page starts with where it stands: status, verdict, severity, host, check and target, when it opened and last fired. Beside the title are **Acknowledge** (stops reminders; the incident stays open and still resolves by itself), **Mute** (no alerts for the time you pick), **Unmute** (lift the mute) and **Dismiss** (close it as a known condition). The dismiss dialog asks why this is not a real problem and how long to keep the check quiet, and spells out what each choice does before you confirm. **Dismiss** appears once the incident is confirmed; a pending incident is not dismissible. The rule a dismiss writes matches the target exactly, so a target containing `*` cannot be given a period: dismiss with **No**, and write the rule by hand under **Known conditions**. Dismissed incidents show up under **Expected**, and their rule appears on the **Known conditions** page.

On a wide screen the page has two columns: the summary, triage, steps by hand, details and chat on the left; the proposed actions, the tools used and the timeline on the right. On a phone it is one column, with the proposed actions straight after the summary. If the incident changes while you have it open — it resolves, or someone acknowledges it in chat — the page updates in place and tells you in a short message.

The page lists **Check by hand** when triage named steps a person does at the machine, for example checking a power cable, a fan or the UPS display. The steps are ordered and short, they are words for you, and nothing here runs anything on the host; the list says it was written by the model. You can tick steps off as you go: the ticks are kept in that browser tab only and are never sent to the hub. The checklist is on this page only: the chat card carries the summary, the cause and the next step, so when an incident was raised in chat, open its page to work through the steps.

**Skills and tools used** lists what triage used, including in follow-up answers, with how many times each ran, how many calls failed and how many were not run because the tool-call budget was spent, and the skills it proposed. Read tools and read skills only look; they never change the host.

The **Timeline** tells the incident's history in words, oldest first: "Opened", "Confirmed", "Triage reported", "Acknowledged", "Resolved", each with its time. A long history shows its newest entries and a **Show all** button.

## The conversation you keep having

You ask follow-up questions in the incident page's **Chat** section, in its **Ask about this incident** box: press Enter to send the question, and Shift+Enter to start a new line in it. The box grows with what you write and holds up to 1000 characters; near the limit it shows how many you used. If triage is off or the monthly budget is spent, the box is disabled and says why. Or ask in the incident's chat thread. Every answer is given the same evidence triage had: the report, the actions on record, the recent timeline, the notes and the host profile, plus the read tools. The model only reads, and it may suggest: an answer never runs anything and never approves anything. When an answer suggests actions, each suggestion names a skill the host offers and becomes a **Proposed** action waiting for you on the **Approvals page**; the answer lists what was suggested, says to approve it there, and says why anything was not suggested. At most three actions come from one answer. The text of logs, alerts, reports, tool results and suggestions is treated as facts to answer from, never as instructions to follow — the same for the questions and answers already remembered, and for your own question.

The earlier questions and answers of that incident stay in memory and the **Chat** section shows them, oldest first, with your question to the right and the answer as text. A question you sent that has no answer yet is shown with "Thinking…" until the answer arrives. If the answer fails, your question stays where it was with the reason under it and a **Retry** button. **Clear conversation** makes the model forget the whole conversation, after you confirm it; the questions you asked are still listed in the incident's timeline. Two questions at once each see the conversation as it was when they started, and questions are replayed in the order you asked them even when the answers come back out of order. A question whose answer is still being written when you clear the conversation is dropped: you still see the answer on screen, but it is never remembered, and the next question is not shown it.
