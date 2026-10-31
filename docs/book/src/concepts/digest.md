# Digest and advisories

## The daily digest

Once a day, at `[notify.digest] time` in `[hub] timezone` (09:00 by default), Domovoi sends one message covering every host. The same lines every morning are noise, so:

- **A host with nothing new is one line:** `pi: healthy, nothing changed`. A host is quiet when it has no open incidents, no advisory raised that day, and no fact moved by more than 1% since the previous digest.
- **Other hosts list what changed,** with the change over the last week: `Pool free 480 GB, down 41 GB in 7d`.
- **Free space that is running out gets a projection,** from a least-squares fit over the last 14 days: `full in ~12 weeks at this rate`. Projections are shown only when they are under 180 days, and a slow decline that will run out within that time is shown even if each day's change is small.
- **Then the open incidents,** one line each with their age and verdict, and the active advisories.

Facts come from checks and skills (disk usage, pool free space, image ages, temperatures).

## Advisories

Advisories are hygiene findings that are worth knowing but not worth waking up for. They appear in the digest and the web UI and never notify on their own. Each one resolves by itself once its condition clears.

| Advisory | Raised when |
| --- | --- |
| Heartbeat missing | No `[heartbeat]` is configured, or its pings have failed for more than 10 minutes. |
| Triage degraded | The LLM provider keeps failing. Alerts still go out without triage. |
| Stale image | A container's image is more than 180 days old. |
| Stack without source | A compose project's files are missing from disk. |
| Secret exposed | A secret is listed in `[secrets] exposed`; rotate it, then remove it from the list. |
| Baseline entry expired | Something expected on a host was gone long enough to leave the baseline. |
| Agent outdated | An agent is a minor version (or more) behind the hub. |
