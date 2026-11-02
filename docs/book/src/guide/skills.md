# Writing skills

A skill is a small program installed on a host, with a manifest that says what it does, what it may change and exactly which arguments it accepts. Skills are the only way Domovoi changes anything:

- The triage agent can run **read** skills on its own while it investigates.
- It can *propose* **write** and **destructive** skills, with arguments. A proposal is only data until a person approves it.
- The agent checks every request against the manifests on its own disk. The hub cannot send it a script, and a model cannot invent a skill or an argument the manifest does not allow.

## The starter skills

The release archive includes seven skills in `examples/skills/` (installed to `/usr/local/share/domovoi/examples/skills`):

| Skill | Risk | What it does | Arguments |
| --- | --- | --- | --- |
| `system_overview` | read | Uptime, load, memory and disk usage | none |
| `top_processes` | read | The 15 processes using the most CPU, and the most memory | none |
| `disk_usage_top` | read | The largest directories under a path | `path`: `/var/log`, `/var/lib/docker` or `/home` |
| `journal_errors` | read | Errors a unit logged in the last few minutes | `unit`, `minutes` (1–1440) |
| `restart_container` | write | `docker restart` one container, then re-check | `container` |
| `restart_unit` | write | `systemctl restart` one unit, then re-check | `unit` |
| `vacuum_journal` | destructive | Delete journal files older than 7, 14 or 30 days | `keep`: `7d`, `14d` or `30d` |

Install them as the [quick start](../quick-start.md#5-install-the-starter-skills) shows. Copy only the ones you want; each folder stands alone.

## Where skills live

Each skill is a folder under the agent's skills directory, `/etc/domovoi/skills` by default (`[agent] skills_dir`):

```text
/etc/domovoi/skills/
  restart_container/
    skill.toml
    run
    README.md
```

- The folder's name must equal the manifest's `name`.
- The entry must be a regular, executable file inside the folder, not writable by group or others (`chmod 755`).
- The folder must not be world-writable. Keep the whole tree owned by root.
- The agent reads the folder when it starts. After adding or changing a skill, run `domovoi check skills /etc/domovoi/skills`, then restart the agent.

Editing a manifest changes its hash, which cancels any pending approval for that skill: you always approve exactly what runs.

## The manifest

```toml
name = "restart_container"
description = "Restart one Docker container, then re-run the check that raised the incident."
risk = "write"
entry = "run"
timeout = "2m"
applies_to = ["container.expected_down", "container.unhealthy", "container.restart_loop"]
max_output_bytes = 8192

[[params]]
name = "container"
description = "The container's name"
kind = { type = "string", pattern = '^[A-Za-z0-9][A-Za-z0-9_.-]{0,127}$', max_len = 128 }

[verify]
recheck = true
delay = "30s"
```

| Key | Meaning |
| --- | --- |
| `name` | `^[a-z][a-z0-9_]{0,63}$`, and the folder's name. |
| `description` | One sentence, shown to people and to the model. Say what it does and when it helps. |
| `risk` | `read`, `write` or `destructive`. See below. |
| `entry` | The program, relative to the folder. It is executed directly, never through a shell. |
| `timeout` | 1 second to 1 hour. The whole process group is killed when it runs out. |
| `max_output_bytes` | 1 KiB to 1 MiB of captured output. |
| `run_as` | A system user to run as instead of the agent's user (root). Optional. |
| `hosts` | Host names it may run on. Empty means any. |
| `applies_to` | Check keys whose incidents it can be proposed for. Empty means any. |
| `[[params]]` | The arguments, in the order they are passed. |
| `[verify]` | For write and destructive skills: re-run the originating check after `delay`, and mark the action verified or failed. |
| `[revert]` | For write skills: `entry` that undoes the change, and `confirm_window` during which a person may ask for it. |

### Parameters

Each parameter has a `name`, a `description`, `required` (default `true`) and a `kind`:

| Kind | Accepts |
| --- | --- |
| `{ type = "string", pattern = '^…$', max_len = 64 }` | Text that fully matches the anchored pattern, at most `max_len` characters |
| `{ type = "integer", min = 0, max = 300 }` | A whole number in the range |
| `{ type = "boolean" }` | `true` or `false` |
| `{ type = "enum", values = ["7d", "14d"] }` | One of the listed strings |

Make patterns as narrow as the skill needs. Start them with a letter or digit, so a value can never look like an option (`-rf`). Prefer `enum` whenever the set of good values is small.

### How the program is called

- argv is the entry followed by one value per declared parameter, in order. An optional parameter that was not given is an empty string.
- The environment is empty except `PATH`, `LANG=C.UTF-8`, `HOME=/nonexistent`, `DOMOVOI_SKILL=<name>` and `DOMOVOI_PARAM_<NAME>=<value>` for each argument.
- It runs in its own process group, with the skill's folder as its working directory.
- Output past `max_output_bytes` is cut off, and secrets the agent knows are redacted before it leaves the host.
- Exit status 0 is success. Anything else, or a timeout, marks the action failed.

POSIX `sh` is fine for the script itself, as long as it starts with a shebang. Quote every argument (`"$1"`), and pass it after `--` where the tool supports that.

## Risk levels and approvals

| Risk | Triage may | A person approves |
| --- | --- | --- |
| `read` | run it on its own while investigating | not needed |
| `write` | propose it with arguments | with **Approve** on the Discord card, or in the web UI |
| `destructive` | propose it with arguments | only in the web UI, by typing the skill's name. Chat never shows an approve button for it. |

The agent also has its own limit: `[policy] allow_risk` in `agent.toml`, `["read", "write"]` by default. A skill above it is refused on that host even if it was approved. Add `"destructive"` only on hosts where you want destructive skills to be possible at all.

Use `write` for changes you could undo, `destructive` for anything that deletes data or cannot be undone.

## Write your own

A skill that flushes a stuck Postfix queue might look like this:

```toml
name = "postfix_flush"
description = "Ask Postfix to retry delivery of every queued message now."
risk = "write"
entry = "run"
timeout = "1m"
applies_to = ["unit.watch"]
max_output_bytes = 8192

[verify]
recheck = true
delay = "1m"
```

```sh
#!/bin/sh
set -eu
postqueue -f
postqueue -p | tail -n 1
```

Then check and load it:

```sh
sudo chmod 755 /etc/domovoi/skills/postfix_flush/run
sudo domovoi check skills /etc/domovoi/skills
sudo systemctl restart domovoi-agent
```

`domovoi check skills` lists each valid skill and explains every problem: a bad pattern, a missing entry, a group-writable script or a folder name that does not match.
