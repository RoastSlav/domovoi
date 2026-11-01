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

## Enrolling agents

### `domovoi hub token create <host>`

Prints a single-use enrollment token for `<host>`, the hub's certificate fingerprint, and the exact command to run on that host. `--ttl` sets how long the token stays valid (default `15m`).

```sh
domovoi hub token create nas --ttl 30m
```

### `domovoi agent enroll`

Run on the new host with the command the hub printed. It trades the token for the agent's identity and pins the hub's certificate. When `agent.toml` does not exist yet, it writes one from `--name` and `--hub` (mode 0640), with defaults for the rest; an existing file is never changed.

```sh
domovoi agent enroll --hub wss://hub.lan:7421 --token <token> --fingerprint AB:CD:… --name nas
```

The fingerprint may be written with or without colons. The token can also come from `DOMOVOI_ENROLL_TOKEN`.

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

`hub token create`, `hub agent list`, `hub agent revoke`, `hub set-password` and `digest now` open the hub's database directly. A running hub holds an exclusive lock on `<data_dir>/hub.lock`, so while it runs these commands refuse with:

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

Pushes one synthetic critical signal through the whole pipeline and prints PASS, FAIL or SKIP for each stage: `config`, `store`, `manager` (an incident opens), `triage` (a report comes back), `card` (the card reaches every gateway) and `gateway` (the incident resolves and every update is delivered).

```sh
domovoi selftest --config /etc/domovoi/hub.toml
domovoi selftest --config hub.toml --offline
```

The run uses a temporary data directory, so it never touches the hub's database and works next to a running hub. Without `--offline` it uses the configured model and chat gateways, which proves delivery works end to end. A real card is posted and then resolved. With `--offline`, a built-in scripted model and a recording gateway stand in, which proves the pipeline without network access.

### `domovoi digest now`

Builds today's digest and sends it to every notifier (`[discord] webhook_url`). The hub must be stopped; with it running, use **Send now** on the web UI's Digest page.

```sh
domovoi digest now --config /etc/domovoi/hub.toml
```

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

`GET /metrics` on the hub's web listener serves Prometheus text, with no login needed:

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
| `domovoi_event_bus_lagged_total` | `consumer` | Times a consumer fell behind the event bus |
| `domovoi_incident_flaps_total` | | Incidents that resolved and fired again quickly |
