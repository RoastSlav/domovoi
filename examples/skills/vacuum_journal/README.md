# vacuum_journal

Deletes archived journal files older than 7, 14 or 30 days with
`journalctl --vacuum-time`, and prints the journal's size before and after.

Deleted logs are gone for good, so this skill is `destructive`: it can only be approved
in the web UI, by typing the skill's name, never from a chat button. Only add it to an
agent whose `[policy] allow_risk` includes `destructive`.

| Parameter | Values |
| --- | --- |
| `keep` | `7d`, `14d` or `30d` |
