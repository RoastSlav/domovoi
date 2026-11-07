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

A known condition says "this firing is expected, so leave it alone". Add one under **Known conditions** in the web UI: the check key, the host it applies to (any host, or one named host), a target pattern (`*` allowed at the ends), why it is acceptable (the note), conditions on the evidence, and how long it lives — 1 hour, 1 day, 1 week, 30 days, or never. Each condition compares one evidence key with a value: equals, contains, is at most, is at least,
or is in network, whose value is one address, such as `192.168.1.20`, or a range in CIDR form,
such as `192.168.1.0/24`. All of them must hold for the rule to match.

Signals that match a known condition are stored, incidents still change, and the history stays complete; nothing is notified and triage does not run for them. The page lists every rule, soonest expiry first, rules that never expire last, and shows expired rules so you can delete them. A rule stops applying on its own when it expires.

A new rule applies to signals that arrive after it is created. An incident that is already open keeps its card and its thread: a rule only holds back the notifications the next signal would cause.

A value that looks like a number in an **equals** condition is compared as a number. `port equals 22` matches a check reporting the number 22; `user equals 1000` does not match evidence stored as the text `1000`. Built-in checks report numbers as numbers, so this is mostly about webhook evidence, where a number arriving as a string needs **contains** rather than **equals**.

You can dismiss an incident instead of writing a rule by hand: on the incident, choose **Dismiss**, say why this is not a real problem, and pick how long the check should stay quiet — 1 day, 1 week, 30 days, always, or **No** to close the incident without a rule, so the check alerts again next time it fires. Dismissal closes the incident right away with the verdict Expected, and the rule it creates matches that incident's host, check and target exactly, so only this one condition goes quiet. Your reason becomes the rule's note, so the **Known conditions** page can say later what it covers.

A dismissed incident that keeps firing reopens within the reopen window, quietly: no alert, no triage, and no notification. It keeps the Expected verdict, so it sits under **Being handled** — nothing is expected of anyone about it. After the reopen window passes, the condition starts a **new** incident: the rule keeps it silent, it has no verdict, and it shows under **Needs attention**. A nightly condition that is dismissed as always-quiet shows a new incident there every night, which is what "quiet" means: not told about it, not counted as something you have triaged.

## What triggers triage

An incident asks for triage when it opens, when it reopens and when its severity rises.

Open an incident with a report and choose **Run triage again** to run the model from scratch with the current skills, notes and evidence, even when a cached report would fit. Before a report exists, the button says **Retry triage**. Both count against the same hourly and monthly caps as any other run.

A run deferred by the hourly cap may re-run on its own once a run slot frees, as long as the incident is still open, unmuted, and not already triaged by a newer request. A failed run retries after an hour; retries for one incident stop after three failed runs, and once the triage-degraded advisory is up, newly failed runs are not deferred at all. Muted incidents are never retried; acknowledged ones still are. Pending retries live in memory, so they are dropped when the hub restarts.

## In the web UI

The **Incidents** page groups what you have by state, newest firing first inside each group:

- **Needs attention** — open incidents nobody has acknowledged, apart from the ones with the verdict Expected. The rule-silenced incidents that a known condition makes are here too, silent and untriaged, with nothing said about them.
- **Being handled** — acknowledged, muted, and open incidents judged expected: dismissed as a known condition, or expected by triage. Muted ones return when the mute ends; dismissed ones stay quiet while their rule holds.
- **Waiting for confirmation** — pending incidents, not confirmed yet. Domovoi alerts if the check keeps failing.
- **Resolved** — the 20 most recently cleared problems, newest first. The group starts closed; **All resolved incidents** opens the rest.

Empty groups are hidden, and **Needs attention** says "Nothing needs attention." when there is nothing to see. If a request fails with nothing on screen, the page says the incidents could not be loaded and shows no groups, so a hub that cannot be read never looks like a quiet night. A refetch that fails over rows already shown keeps them and notes "Could not refresh." The host and severity filters apply to every group. Choose a status in the filters to get one state as a single page with paging.

An incident's own page offers **Ack** (stops reminders), **Mute** (silence for the time you pick), **Unmute** (lift the mute) and **Dismiss** (close it as a known condition, with your reason and how long to keep the check quiet). **Dismiss** appears once the incident is confirmed; a pending incident is not dismissible. The rule a dismiss writes matches the target exactly, so a target containing `*` cannot be given a period: dismiss with **No**, and write the rule by hand under **Known conditions**. Dismissed incidents show up under **Being handled**, and their rule appears on the **Known conditions** page. Below the actions, the page lists the skills and tools triage used, including in follow-up answers, with how many times each ran and how many calls failed, and the skills it proposed. Read tools and read skills only look; they never change the host.
