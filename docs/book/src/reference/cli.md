# Command line

One binary, `domovoi`, runs the hub, the agent, or both, and has tools for setting up and checking an installation.

Global options, accepted by every command:

| Option | Meaning |
| --- | --- |
| `--log-format human\|json` | How log lines look on standard error. Default `human`. |
| `--log-level <filter>` | A filter such as `info` or `domovoi_hub=debug`. Overrides `RUST_LOG`. The default is `info` for `hub run` and `agent run`, and `warn` for every other command. |

Hub commands read `--config` (short `-c`) or `DOMOVOI_HUB_CONFIG`. Both are optional: without them, `/etc/domovoi/hub.toml` is read when it exists, and otherwise the hub uses defaults, `DOMOVOI_*` variables and the settings saved in the web UI. Every hub command sees the settings saved in the UI, except `check config`, which checks only the file. See [Advanced: config as code](../install/config-as-code.md). Agent commands read `--config`/`DOMOVOI_AGENT_CONFIG`, which defaults to `/etc/domovoi/agent.toml`.

Exit codes: `0` on success, `1` on failure (an invalid file, a failed stage, a refused command), `2` for a mistyped command line.

## Running

### `domovoi hub run`

Runs the hub until SIGINT or SIGTERM, then shuts down within 15 seconds.

```sh
domovoi hub run
domovoi hub run --config /etc/domovoi/hub.toml
```

On a new hub with no account yet, it prints a setup link valid for 24 hours. Open it to create the admin account and go through the guided first run.

When settings are saved in the web UI, `hub run` checks them, answers the page, and then restarts the hub in the same process on the same data directory. The restart takes a few seconds; agents reconnect and flush what they queued. If the hub does not start with the new settings, it goes back to the previous ones and raises a `SettingsApplyFailed` advisory. The secret key for settings entered in the UI is created at first start as `<data_dir>/secret.key` (mode 0600); the hub refuses to start if others can read it.

### Single-box mode: `domovoi hub run --with-local-agent`

Runs the hub and an agent for the same machine in one process.

```sh
domovoi hub run --config hub.toml --with-local-agent agent.toml
```

The agent needs no enrollment token, because it shares the hub's process. It generates its key on first start under `[agent] state_dir` and is registered for the hub's own host. It then talks to the hub over an in-memory link that carries the same protocol and handshake as the network link. Its `hub_url` is not used, but must still be a valid `wss://` URL.

### `domovoi agent run`

Runs the agent until SIGINT or SIGTERM. Skills and check commands run through the sandboxed executor.

```sh
domovoi agent run --config /etc/domovoi/agent.toml
```

### `domovoi agent status`

Prints what the link last recorded: whether it is connected, reconnecting, or idle because the hub speaks another protocol version. For an incompatible agent it names both protocols and the file's update time. The file is only as fresh as the last write, so a stopped agent leaves a stale line.

```sh
domovoi agent status --config /etc/domovoi/agent.toml
```

Without a status file it prints `no status recorded; the agent has not run yet.`

### `domovoi agent selftest`

Checks the host's identities, modes, units and isolation, and says which platform tier the result gives. It is not `domovoi selftest`, which tests the hub pipeline.

```sh
domovoi agent selftest --config /etc/domovoi/agent.toml
sudo domovoi agent selftest --installer --record
```

| Flag | Meaning |
| --- | --- |
| `--installer` | Run as root. Runs the root checks itself and the agent's own checks as `domovoi-agent`, then merges both. |
| `--record` | With `--installer`, write the result to `/var/lib/domovoi-helper/platform.json`. The helper reads that record to decide about write groups. |
| `--json` | Print the full report as JSON instead of text. |
| `--config <agent.toml>` | The agent config to read. Default `/etc/domovoi/agent.toml`. |

Without `--installer` it runs the checks the agent's account can run (ST-01, ST-02, ST-12, ST-14, ST-15, ST-16) and reports the rest as not run because they need root. The output has one line per check, such as `ST-10 fail path_mode_wrong`, then each fault with the found and the expected mode and owner, then the privilege-boundary label and the tier decision. It also lists skills whose manifest names a `run_as` other than `domovoi-skill`.

Exit code `0` when no check failed, `1` when any failed. A check that was not run is printed as `nd` and is never counted as a pass. It never repairs anything.

| Check | What it looks at | Token on failure |
| --- | --- | --- |
| ST-01 | the agent runs as `domovoi-agent`, not root | `privilege_boundary_not_in_effect` |
| ST-02 | the agent's groups are its own plus `systemd-journal`, never `docker` or root | `agent_groups_unexpected` |
| ST-03 | `domovoi-skill` exists, is not root or the agent, has no login shell and no extra group | `skill_account_invalid` |
| ST-04 | neither account is in the `docker` group | `docker_group_member` |
| ST-05 to ST-06 | the skill runner's groups and privileges (not run until the runner ships) | `runner_groups_extra`, `runner_privileges` |
| ST-07 | the helper and agent units are as shipped: no drop-in, no pending reload | `unit_modified`, `unit_not_inspectable` |
| ST-08 | records the systemd version | none |
| ST-09 | no numeric ids in units, no `ExecStart` under `/run`, `/tmp` or `/dev/shm` | `unit_hardcoded_id`, `unit_exec_under_run` |
| ST-10 | every path's mode, owner and type, and no write bit in the skills and packages trees | `path_mode_wrong` |
| ST-11 | the helper's directory, lock, key, state and record | `helper_state_permissions` |
| ST-12 | the helper socket is `root:domovoi-agent`, mode 0660 | `socket_mode_wrong` |
| ST-13 | the skill runner's isolation (not run until the runner ships) | `runner_not_isolated` |
| ST-14 | the agent can connect to the helper and get `health` | `socket_unreachable` |
| ST-15 | the agent cannot open the helper's key, directory or lock | `agent_reaches_helper_state` |
| ST-16 | the agent cannot connect to the Docker socket | `agent_reaches_docker` |
| ST-17 | container installs only: no risky mounts | `container_escape_surface` |
| ST-18 | `/etc/domovoi` was not created empty by a unit mask | `host_directory_created_by_mask` |
| ST-19 | records security modules and mount options | none |
| ST-20 | state writes are not slow (a warning only) | `state_write_slow` |

### `domovoi helper serve`, `init` and `repair`

The helper is the root part of the three-identities layout. See [Helper](helper.md) for what it answers and how it is configured.

`domovoi helper serve` is started by the helper's socket unit, adopts the connection from standard input, answers one request and exits. It refuses to run when standard input is not a socket, so do not run it by hand.

`sudo domovoi helper init` creates `/var/lib/domovoi-helper` (mode 0700) and its lock file. It prints `created` or `already initialised`, and refuses, changing nothing, when the directory exists with other modes.

`sudo domovoi helper repair` is the only command that changes a wrong mode or owner on a Domovoi path. It prints each path with the found and the expected mode, owner and group and the exact change, then asks you to type `repair` at the terminal. There is no `--yes`: without a terminal it changes nothing. It refuses symlinks and paths of the wrong type, changes only the paths it lists and only their mode and owner, never content, and then re-runs the checks and prints what is left.

## Enrolling agents

### `domovoi hub token create <host>`

Prints a single-use enrollment token for `<host>`, the hub's certificate fingerprint, and the exact command to run on that host. `--ttl` sets how long the token stays valid (default `15m`).

```sh
domovoi hub token create nas --ttl 30m
```

### `domovoi agent enroll`

Run on the new host with the command the hub printed. It trades the token for the agent's identity and pins the hub's certificate. When `agent.toml` does not exist yet, it writes one from `--name` and `--hub` (mode 0644, so the unprivileged agent can read it), with defaults for the rest. When `agent.toml` already exists, only its `hub_url` is changed, to `--hub` if they differ; every other setting is kept.

```sh
domovoi agent enroll --hub wss://hub.lan:7421 --token <token> --fingerprint AB:CD:… --name nas
```

The fingerprint may be written with or without colons. The token can also come from `DOMOVOI_ENROLL_TOKEN`.

When it runs as root on a host where the `domovoi-agent` account exists, it hands the files in `state_dir` over to that account after its last write and prints one line saying so. `agent.toml` stays owned by root. `domovoi agent journal init` and `reset` do the same.

### `domovoi hub agent list` and `domovoi hub agent revoke <host>`

`list` shows every host with its enrolled agent's version and enrollment time. `revoke` revokes a host's agent; it can never connect again until the host is enrolled anew.

```sh
domovoi hub agent list
domovoi hub agent revoke old-nas
```

### `domovoi hub set-password`

Sets the web UI password. It prompts twice without echo. Use `--password-stdin` to read it from the first line of standard input. `--username` picks the account; the default is the first (admin) account. On a hub with no account yet, it creates the admin account (named `admin` unless `--username` says otherwise). Passwords need at least 12 characters.

```sh
domovoi hub set-password
printf '%s\n' "$NEW_PASSWORD" | domovoi hub set-password --password-stdin
```

### Commands that need the hub stopped

`hub token create`, `hub agent list`, `hub agent revoke`, `hub set-password`, `digest now`, `audit verify` and `audit head` open the hub's database directly. A running hub holds an exclusive lock on `<data_dir>/hub.lock`, so while it runs these commands refuse with:

```text
error: a hub is already running on /var/lib/domovoi; stop it or use the web UI instead
```

The web UI does all of these while the hub runs. The lock is released when the hub exits, crash included.

## Checking an installation

### `domovoi check config <file>`

Loads and validates a configuration file without starting anything. It prints every problem with its path in the file (for example `error: llm.primary.api_key: …`) and exits 1 if there are errors. The kind of file is guessed from its `[hub]` or `[agent]` table; `--kind hub|agent` says it explicitly. Secrets given as `{ env = … }` or `{ file = … }` are resolved, so a missing variable is reported too.

```sh
domovoi check config /etc/domovoi/hub.toml
domovoi check config agent.toml --kind agent
```

### `domovoi check skills <dir>`

Loads a skills folder (one sub-folder per skill, each with a `skill.toml`) and lists the valid skills and every problem. Exits 1 if any skill has a problem.

```sh
domovoi check skills /etc/domovoi/skills
```

### `domovoi check run`

Runs every enabled check once on this host and prints what each one saw, plus its digest facts. No hub is needed, and the agent's stored check state is not touched. `--only <check>` runs a single check, and `--json` prints JSON. A check that cannot run here, for example because `smartctl` is not installed or because the agent runs in a container and the check needs journalctl, is reported as `unavailable`. A check that fails for another reason is reported as `error`. The command still exits 0, because the run itself completed.

```sh
domovoi check run --config /etc/domovoi/agent.toml
domovoi check run --only disk.usage --json
```

### `domovoi notify test`

Sends a test card to each enabled chat gateway and a test message to each notifier, with PASS or FAIL for each. It works next to a running hub.

```sh
domovoi notify test --config /etc/domovoi/hub.toml
```

### `domovoi selftest`

Tests the hub, not the host (for the host, see `domovoi agent selftest`). It pushes one synthetic critical signal through the whole pipeline and prints PASS, FAIL or SKIP for each stage: `config`, `store`, `manager` (an incident opens), `triage` (a report comes back), `card` (the card reaches every gateway) and `gateway` (the incident resolves and every update is delivered).

```sh
domovoi selftest --config /etc/domovoi/hub.toml
domovoi selftest --config hub.toml --offline
```

The run uses a temporary data directory, so it never touches the hub's database and works next to a running hub. Without `--offline` it uses the configured model and chat gateways, which proves delivery works end to end. A real card is posted and then resolved. With `--offline`, a built-in scripted model and a recording gateway stand in, which proves the pipeline without network access.

### `domovoi digest now`

Builds today's digest and sends it to every notifier: the `[discord] webhook_url` if set, otherwise the bot. The hub must be stopped; with it running, use **Send now** on the web UI's Digest page.

```sh
domovoi digest now --config /etc/domovoi/hub.toml
```

### `domovoi audit verify`

Verifies the audit log's hash chain and the anchor that commits to the rows written before the chain existed. Without `--last` it checks the whole history: the legacy digest and every retained chained row. `--last <n>` checks the legacy row count only and the newest n chained rows.

It prints the database it read, then one line for each part:

```text
Audit log of /var/lib/domovoi/hub.db
Legacy history:  verified, 1204 rows (digest matches the anchor)
Chained history: intact, 311 rows checked, head seq 1515 hash 9f0e00...
```

It exits 0 when both parts hold, and 1 when a chained row was edited, deleted or inserted, or when the legacy history no longer matches the anchor. Legacy rows that were pruned are reported as `not verifiable` and do not fail the command.

```sh
domovoi audit verify --config /etc/domovoi/hub.toml
domovoi audit verify --last 1000
```

### `domovoi restore <backup>`

Installs a backup as the hub's database. It needs the hub stopped: the command takes the data directory's lock and refuses with `a hub is running on <dir>; stop it before restoring` while one holds it. It is an offline command; the web UI cannot restore.

```sh
domovoi restore /var/lib/domovoi/backups/domovoi-20261114T040000Z.db          # describe only
domovoi restore /var/lib/domovoi/backups/domovoi-20261114T040000Z.db --yes    # do it
```

Without `--yes` it verifies the file in full and prints what would happen, then exits with code 2: nothing changes.

With `--yes` it prepares the backup before touching the live database, and only then installs it:

1. verifies the backup in full;
2. copies it beside the database as `hub.db.restore-tmp`;
3. makes the copy owner-only on Unix, and syncs the file and, on Unix, its directory;
4. opens the copy and runs any pending migrations on it, so a backup older than this release is brought up to date before it becomes live;
5. moves the current `hub.db` and its `-wal` and `-shm` to `hub.db.before-restore-<time>`;
6. renames the prepared copy into place.

A backup that cannot be opened or migrated therefore leaves the live database untouched. The previous database is never deleted. If installing the prepared copy fails, every move is reversed; if that reversal cannot finish, the command fails with an error naming each file that is still out of place.

A rename is only durable once its directory is flushed, so on Unix the command flushes the directory three times: after preparing the copy, after moving the previous database aside, and after installing the new one. A failed flush is an error, and the stage it belongs to is undone — the backup is never installed when the previous database could not be recorded as moved aside, and the previous database is put back if the install could not be recorded. If the flush that would record the rollback fails too, the command says the result is not durable rather than reporting a clean failure.

On Windows there is no portable directory fsync, so those flushes cannot run and do not fail; the final rename is not durable against a power loss, and the preserved `hub.db.before-restore-<time>` is what to fall back to. A database that will not open is kept as `hub.db.failed-restore-<time>`, or at `hub.db.restore-tmp` on Windows where the failed open still holds it.

The restore is not crash-atomic. What it guarantees is that no failure deletes your previous database, and that any uncertain state is named in the error.

| Argument | Meaning |
| --- | --- |
| `<BACKUP>` | The backup file to install. |
| `--config <path>` | The hub's `hub.toml`; only its data directory is used. It defaults like every other command. |
| `--yes` | Restore without asking. |

It refuses a file that is damaged, is not a Domovoi database, has a migration that did not finish, was written by a newer release, or records a schema version newer than this binary can read. It changes nothing in that case.

| Exit code | Meaning |
| --- | --- |
| 0 | Restored. |
| 1 | Refused or failed; nothing changed, or the previous database was put back. |
| 2 | Not confirmed: run again with `--yes`. |

Afterwards, restore `secret.key` and `tls/` from the same time if they were lost, start the hub, and run `domovoi audit verify`.

### `domovoi audit head`

Prints the newest chained audit row's sequence number and hash, or `no chained rows yet` when the chain has not started.

```sh
domovoi audit head --config /etc/domovoi/hub.toml
```

Run it after a backup, or on a schedule, and keep the output off the host. Comparing it with a later run shows whether the newest audit rows were deleted, which the chain alone cannot detect.

## Other

### `domovoi version`

Prints the version, git commit, build date and target. `domovoi` with no command does the same.

```text
domovoi 0.1.0
commit:  2bfe62833d2f
built:   2026-11-01
target:  x86_64-unknown-linux-gnu
```

Builds from a source archive without `.git` print `unknown` as the commit, unless `DOMOVOI_GIT_COMMIT` is set at build time. `SOURCE_DATE_EPOCH` fixes the build date for reproducible builds.

## Metrics

`GET /metrics` on the hub's web listener serves Prometheus text. It needs a logged-in web session, or a bearer token configured in `hub.toml`:

```toml
[metrics]
token = { env = "DOMOVOI_METRICS_TOKEN" }
```

The token is a secret like any other, so it comes from the environment or a file, never inline, and changing it in the web UI needs the admin's password again. Without a token, only a session reads the page; with one, a scraper sends it as `Authorization: Bearer <token>`. Anything else gets `401` with no metrics text.

```yaml
scrape_configs:
  - job_name: domovoi
    scheme: https
    static_configs:
      - targets: ["domovoi.example.net:7420"]
    authorization:
      type: Bearer
      credentials_file: /etc/prometheus/domovoi-metrics-token
```

| Metric | Labels | Meaning |
| --- | --- | --- |
| `domovoi_signals_total` | `source` | Signals received (`builtin`, `webhook`, `probe`, `agent`) |
| `domovoi_incidents` | `status` | Active incidents (`pending`, `open`, `acked`, `muted`) |
| `domovoi_triage_runs_total` | `outcome` | Triage results (`report`, `cached`, `skipped`, `failed`) |
| `domovoi_llm_tokens_total` | `provider`, `direction` | Model tokens (`input`, `output`) |
| `domovoi_llm_cost_usd_total` | | Model cost in US dollars, as reported by the provider |
| `domovoi_actions_total` | `status` | Action status changes |
| `domovoi_agents_connected` | | Agents with a live session |
| `domovoi_delivery_failures_total` | `gateway` | Chat deliveries that failed after retries |
| `domovoi_notify_state_errors_total` | `op` | Notification state reads and writes that failed (`read`, `write`) |
| `domovoi_event_bus_lagged_total` | `consumer` | Times a consumer fell behind the event bus |
| `domovoi_incident_flaps_total` | | Incidents that resolved and fired again quickly |
| `domovoi_backup_last_success_timestamp_seconds` | | When the nightly backup last succeeded |
