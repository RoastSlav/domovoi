# Writing skills

A skill is a small program installed on a host, with a manifest that says what it does, what it may change and exactly which arguments it accepts. Skills are the only way Domovoi changes anything:

- The triage agent can run **read** skills on its own while it investigates.
- It can *propose* **write** and **destructive** skills, with arguments. A proposal is only data until a person approves it.
- The agent checks every request against the manifests on its own disk. The hub cannot send it a script, and a model cannot invent a skill or an argument the manifest does not allow.

## The starter skills

The release archive includes eleven skills in `examples/skills/` (installed to `/usr/local/share/domovoi/examples/skills`):

| Skill | Risk | What it does | Arguments |
| --- | --- | --- | --- |
| `system_overview` | read | Uptime, load, memory and disk usage | none |
| `top_processes` | read | The 15 processes using the most CPU, and the most memory | none |
| `disk_usage_top` | read | The largest directories under a path | `path`: `/var/log`, `/var/lib/docker` or `/home` |
| `journal_errors` | read | Errors a unit logged in the last few minutes | `unit`, `minutes` (1–1440) |
| `power_status` | read | Mains, battery and UPS state of this host | none |
| `hardware_health` | read | Temperatures, recent kernel hardware errors and disk health summary | none |
| `log_summary` | read | Warnings and errors per program over the last 24 hours, and the 20 most recent errors | none |
| `service_list` | read | Running and failed systemd services, and enabled timers | none |
| `restart_container` | write | `docker restart` one container, then re-check | `container` |
| `restart_unit` | write | `systemctl restart` one unit, then re-check | `unit` |
| `vacuum_journal` | destructive | Delete journal files older than 7, 14 or 30 days | `keep`: `7d`, `14d` or `30d` |

The agent installer copies them into `/etc/domovoi/skills`. Each folder stands alone: to stop one from running, delete its folder and restart the agent. The installer adds a deleted starter skill back on the next upgrade, so to keep one removed, edit it instead (for example, set `risk` to a level your agent does not allow).

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

The Skills page lists the skills of the host chosen in the host selector. Under **All hosts** it asks you to pick one. Each row of the table shows:

- the skill's name and description,
- its **Risk**, with the harmless skills first and destructive skills marked,
- its **Timeout**, how long one run may take before the agent stops it,
- **Applies to**, the checks the skill is suggested for, or "Any check" when it names none,
- **Last used**, when Domovoi last sent the skill to that host's agent, or "Never",
- an **Enabled** switch.

Choose a skill's name (also on the Overview tab of a host's page) to open its details: what its risk means, its timeout, what it applies to, its verification and its manifest hash, then a **Parameters** table with the type, constraints and description of every parameter, the folder's `README.md` when it has one, and its **Recent runs** on that host. Each run links to its incident.

You can also turn a skill off without touching the host. Switch **Enabled** off in its row: the skill stays on the host and in the list, shown greyed and tagged "Disabled", and the hub stops running it and stops offering it to triage. The switch is saved per host, so it survives a hub restart, and switching it on again turns the skill back on. The Capabilities page shows the same state for every skill on that host, and a raised floor there can only make a skill stricter.

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
| `run_as` | A system user to run as instead of the agent's user. Optional. On an agent that runs as root, any user. On a migrated agent only `domovoi-skill` is accepted, and a skill that names another user is refused. See [What a skill can reach](#what-a-skill-can-reach). |
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

### What a skill can reach

On a host with the three-identities layout (the agent runs as `domovoi-agent`), every skill runs through the skill runner, as the `domovoi-skill` account. The agent never runs a skill itself there. If the runner is missing or not healthy, skills are unavailable; nothing falls back to running them as the agent or as root.

A skill can:

- read its own folder, and the rest of the skills and packages directories, read-only;
- write to a private `/tmp` that exists only for that run;
- read what any unprivileged account can read on the host.

A skill cannot reach the agent's state, the helper's directory or key, `/etc/domovoi/agent.toml` and `helper.toml`, or either socket. They do not exist in its view. It has no `docker` group and no root. A skill that needs more asks the helper, which this release does not offer for writes.

Because of this, three things differ from a root agent:

- The working directory is the skill's own folder, **read-only**. Write scratch files under `/tmp`.
- `HOME` points to a directory that does not exist. Do not rely on `~`.
- `run_as` other than `domovoi-skill` stops working. The runner refuses it with `run_as_unsupported`, and `domovoi agent selftest` lists the skill. Remove the `run_as` line, or move the privileged part to the helper when it supports it.

A container agent has no skill runner. Skills are not run there, and the hub refuses them with `agent_in_container` ([container agent](../install/container-agent.md#what-a-container-agent-can-and-cannot-do)). The one exception is the legacy root behaviour below, which does not apply to the container image.

Skills in an agent that still runs as root keep the old behaviour, labelled "privilege boundary not in effect", including `run_as`.

The starter skills `restart_container`, `restart_unit` and `vacuum_journal`, and the readers of the journal and other protected state, need access the `domovoi-skill` account does not have. On a migrated host they report errors until the helper has operations for them. The other starter skills only read what an unprivileged account can read.

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

## Packages

A package bundles skills into one signed archive that the hub can install on an agent. Installing a package does not run its skills. They are inert until the skill runner that executes packaged skills is available, and they are not offered to triage or listed with the skills above. See [Packages](../reference/agent.md#packages) for the layout and trust rules.
