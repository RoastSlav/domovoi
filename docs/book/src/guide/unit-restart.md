# Restarting a service

`unit.restart` restarts one systemd service on a host. You can approve it in the web UI, or a policy rule can allow it to run without you. Both paths end in the same place: the host's own agent and helper decide whether it runs.

## What an approval binds

The approval card names the unit as systemd knows it (an alias is never bound) and binds:

- the unit file and every drop-in file, by content hash;
- what a restart will run, as loaded now: the command, the user and the working directory. Environment values are hidden;
- that the unit lives in `/etc` or `/usr/lib` and is not masked, linked, transient or generated, and that no drop-in exists on disk that systemd has not loaded.

It does not bind the contents of `EnvironmentFile=` files or of the programs the unit runs, and it does not bind other units that restart or stop with this one. The card lists the followers it can see.

A small window remains between the last check and the moment systemd acts, because a user with root can change things inside it. It cannot be closed from here. After the restart Domovoi checks the new process and raises an alert if it is not running what you approved. It never restarts or stops it again.

A restart can fail and leave the service stopped, for example when systemd's start limit is hit. Restarting a stopped service starts it.

## What you set up

Three local files decide whether a restart can run. The hub cannot change any of them.

| Where | Setting | Effect |
| --- | --- | --- |
| `helper.toml` | `[systemd] restart_units` | The units the helper may restart. Empty means none. |
| `helper.toml` | `[safety] protected_units` | Units no restart may touch. The installer's list is always included. |
| `agent.toml` | `[unattended]` | What may run without you: one `[[unattended.allow]]` entry for `unit.restart` with the unit ids and `max_per_hour`. Without it, nothing runs on a policy's say-so. |

An Untested platform, which every platform is today, also needs `allow_write_groups_on_untested = true` in `helper.toml`. The approval card says so.

## When it is refused

| What you see | What it means and what to do |
| --- | --- |
| `target_stale` | systemd has a newer unit file than the one it loaded. Run `systemctl daemon-reload` yourself, then ask again. Domovoi never reloads for you. |
| `read_failed` | The helper could not read the unit. Nothing ran. Check that `busctl` works for the helper and ask again. |
| `target_protected` | The unit is in `protected_units`. |
| `target_not_allowlisted` | The unit is not in `restart_units`. |
| `target_changed` | The unit changed after you approved. Nothing ran. Ask again to approve what it is now. |
| `target_unsupported` | The unit is masked, linked, transient or generated, or is a template instance. |
| `restart_budget_exhausted` | The helper's hourly or per-unit limit is used up. |

## After a restart

A restart that began and did not end well is a failed step, and it counts toward the rule's circuit breaker:

- `start_limit_hit`: the unit is left failed. Domovoi never retries and never calls `reset-failed`.
- `no_restart_observed`: the process did not change.
- `wrong_start`: the process does not match what you approved, for example its environment differs. The result names the fields, never the values. Look at the unit before you trust it.

## An unknown outcome

If the link breaks after the restart was sent, the step stays unknown: Domovoi cannot tell whether the service restarted. It will not issue another permit for that step. Look at the host, then record what you found with `POST /api/v1/operations/{id}/steps/{step}/resolve-unknown`, which needs your session and your password. The web UI has no button for it.
