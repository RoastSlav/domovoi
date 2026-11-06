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

## What triggers triage

An incident asks for triage when it opens, when it reopens and when its severity rises.

Open an incident with a report and choose **Run triage again** to run the model from scratch with the current skills, notes and evidence, even when a cached report would fit. Before a report exists, the button says **Retry triage**. Both count against the same hourly and monthly caps as any other run.

A run deferred by the hourly cap may re-run on its own once a run slot frees, as long as the incident is still open, unmuted, and not already triaged by a newer request. A failed run retries after an hour; retries for one incident stop after three failed runs, and once the triage-degraded advisory is up, newly failed runs are not deferred at all. Muted incidents are never retried; acknowledged ones still are. Pending retries live in memory, so they are dropped when the hub restarts.

## In the web UI

The **Incidents** page groups what you have by state, newest firing first inside each group:

- **Needs attention** — open incidents nobody has acknowledged.
- **Being handled** — acknowledged and muted incidents. Muted ones return when the mute ends.
- **Waiting for confirmation** — pending incidents, not confirmed yet. Domovoi alerts if the check keeps failing.
- **Resolved** — the 20 most recently cleared problems, newest first. The group starts closed; **All resolved incidents** opens the rest.

Empty groups are hidden, and **Needs attention** says "Nothing needs attention." when there is nothing to see. If a request fails with nothing on screen, the page says the incidents could not be loaded and shows no groups, so a hub that cannot be read never looks like a quiet night. A refetch that fails over rows already shown keeps them and notes "Could not refresh." The host and severity filters apply to every group. Choose a status in the filters to get one state as a single page with paging.
