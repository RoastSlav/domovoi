# Migrating from cron scripts

Many homelabs start with a few cron jobs: one checks free space, one pings a service, one sends a message to a chat webhook when something fails, and maybe one pipes logs into an LLM. Domovoi replaces the glue around those scripts, but keeps what is specific to your setup. Move over one script at a time and run both side by side until you trust it.

## 1. Map each check

For each script, find the closest [built-in check](../reference/checks.md):

| Your script checks | Use |
| --- | --- |
| free space with `df` | `disk.free`, with per-mount thresholds |
| a container is running, or healthy | `container.expected_down`, `container.unhealthy`, `container.restart_loop` |
| a systemd service is active | `unit.failed` for any unit, `unit.watch` for a list you name |
| memory, swap, temperatures | `memory.available`, `temperature.cpu` |
| disk health with `smartctl` | `smart.health` |
| a port or URL answers | `probe.tcp`, `probe.http`, run from the hub |
| SSH logins, sudo failures | `security.ssh_login`, `security.sudo_failure` |

Built-in checks keep state for you: they open one incident per problem, confirm before alerting, re-notify on a schedule, and resolve when the problem goes away. Thresholds are set per host on the host's **Checks** tab.

## 2. Keep the checks that are yours

Some checks only make sense on your machines: a backup job's exit code, a certificate on a device, an application's queue length. Keep the script, and replace its chat message with a webhook to Domovoi:

```sh
#!/bin/sh
# /usr/local/bin/backup-nightly, run from cron
if restic backup /srv/photos; then
    status=resolved
else
    status=firing
fi
curl -fsS -X POST http://127.0.0.1:7422/hooks/generic \
    -H 'Content-Type: application/json' \
    --data "{\"check\":\"backup.nightly\",\"target\":\"photos\",\"status\":\"$status\",\"severity\":\"warning\",\"summary\":\"Nightly backup of /srv/photos\"}"
```

This posts to the agent's [local relay](webhooks.md#agent-relay), so the script needs no route or credentials for the hub. Sending `resolved` closes the incident. The same payload can go straight to a [hub webhook](webhooks.md#generic-json) instead.

Use a stable `check` and `target`: together they decide which incident a signal belongs to. Put changing values, such as an exit code or a size, in `evidence`.

## 3. Turn fixes into skills

If a script also *fixes* things (restarts a container, clears a cache, rotates logs), turn the fix into a [skill](skills.md). Triage can then propose it on the right incident, and it runs only when you approve. Keep the arguments narrow: an `enum` of known good values, or a strict pattern.

Fixes that delete data are `destructive`: they can only be approved in the web UI, and only on hosts that allow them.

## 4. Bring your notes

Runbooks help the triage agent: it searches your notes while it investigates. On **Notes**, choose **Import** and select a folder of Markdown files, optionally for one host. Each file's first `# ` heading becomes its title. Importing the same folder again adds nothing. The API equivalent is `POST /api/v1/notes/import`; see [HTTP API](../reference/api.md#importing-notes).

## 5. Run both side by side

Keep your old scripts running for a week or two, but point their chat messages to a separate channel. Compare: did Domovoi raise everything the scripts did, and was each card's verdict right? Then disable the old jobs one at a time.

If Domovoi was noisier, raise thresholds on the host's **Checks** tab, or mute the target. If it missed something, add a check or a webhook for it.
