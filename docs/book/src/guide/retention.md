# Retention

Domovoi keeps raw data for a limited time and keeps the records that explain what happened. Every period below is a setting in `[retention]`, in **Settings → Retention** once the page ships, in `hub.toml`, or through a `DOMOVOI_RETENTION__<KEY>` variable. The defaults are the owner's decision.

| Setting | Default | Removed after | Kept |
| --- | --- | --- | --- |
| `signals_days` | 30 days | signals | |
| `incident_timeline_days` | 90 days | every incident timeline entry except follow-up conversations | |
| `conversations_days` | 180 days | follow-up questions, answers and their clear markers on an incident | |
| `notifications_days` | 30 days | the notification state of a resolved incident | held alerts and the state of open incidents |
| `triage_cache_days` | 90 days | cached triage reports | |
| `resolved_advisories_days` | 30 days | advisories, counted from resolving | open advisories |
| `operation_journal_days` | 180 days | not yet: applies once operations exist | |
| `execution_outputs_days` | 30 days | not yet: applies once execution outputs are stored apart from the audit log | |
| `retracted_facts_days` | 30 days | not yet: applies once knowledge facts exist | |

Operations and their resolution summaries are kept, and so is the audit log: neither has a day count here. Incidents, digest facts and `llm_usage` are kept as well. The 4 KiB output excerpt recorded on an action's audit event belongs to the audit log and is kept with it.

Values must be between 1 and 3650 days. Zero is refused, so a mistyped setting cannot delete everything at the next purge. A key set in `hub.toml` or the environment is locked in the settings page, which shows where each value came from.

The purge runs daily at 03:30 local time. All of its deletes happen in one transaction, so a failure deletes nothing and the next day's run tries again.

## What deletion guarantees

A purge removes the data from the live database, from its full-text index and from what the model sees at that moment. It does not remove it from backups: deleted rows stay in the nightly backups until they rotate out (`[backup] keep`, default 7) and in the last pre-migration copies (`[backup] keep_pre_migrate`, default 3). Restoring a backup brings the data back until the next purge deletes it again.

Changing any retention setting asks for the admin's password again, because a shorter period destroys data sooner.
