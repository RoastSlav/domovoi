# Unattended install

This page is for a script or an automation agent that installs Domovoi without a person at the terminal. It runs the same installer as [Binary and systemd](binary.md), with flags that remove every question and make the output machine-readable.

## A hub on one machine

Run as root. Create the admin password file with mode 0600, preview the plan, then install:

```sh
curl -fsSL https://github.com/RoastSlav/domovoi/releases/latest/download/install.sh -o install.sh
install -m 0600 /dev/null /root/admin.pw
printf '%s\n' "$ADMIN_PASSWORD" > /root/admin.pw   # at least 12 characters

sh install.sh --role hub --dry-run --json --yes \
  --bind lan --public-url http://192.168.1.50:7420 --admin-password-file /root/admin.pw
sh install.sh --role hub --yes --json \
  --bind lan --public-url http://192.168.1.50:7420 --admin-password-file /root/admin.pw
```

The installer installs the binary, the user, the directories and the unit, creates the account `admin`, starts the hub and waits until `GET /readyz` answers. Use `--role both` to watch this machine as well: it also enrolls and starts an agent for it.

Verify:

```sh
curl -fsS http://192.168.1.50:7420/readyz
sudo -u domovoi domovoi selftest --offline
```

`/readyz` answers 200 when the hub can do its job. `domovoi selftest --offline` pushes a synthetic incident through the whole pipeline with a scripted model and no network; drop `--offline` once Discord and a model are configured.

## Another host

On each machine to watch, an agent needs a single-use token (valid 15 minutes), the hub certificate's SHA-256 fingerprint and the agent server address. Get them from the hub's **Hosts**, **Add host** dialog. `domovoi hub token create <host>` prints the same, but it opens the database, so it works only while the hub is stopped (see [the command line](../reference/cli.md#commands-that-need-the-hub-stopped)).

The agent server address is `wss://HOST:7421`, where `HOST` is the one the hub installed with `--agent-url` (by default the host of the public URL). On the agent host, save the token with mode 0600 and run the installer:

```sh
install -m 0600 /dev/null /root/enroll.token
printf '%s\n' "$TOKEN" > /root/enroll.token

sh install.sh --role agent --yes --json \
  --hub wss://192.168.1.50:7421 --fingerprint "$FINGERPRINT" \
  --token-file /root/enroll.token --name nas
```

The installer enrolls the agent, starts it and waits until it is connected. The token can also come from the environment (`DOMOVOI_ENROLL_TOKEN`); a literal `--token VALUE` is refused. Then check the **Hosts** page, or run `sudo domovoi agent status` on the host.

## Upgrade

Run the installer again with the same role, or with `--version vX.Y.Z` to pick the release:

```sh
sh install.sh --role hub --yes --json
sh install.sh --role agent --yes --json
sudo systemctl restart domovoi-agent
```

A hub that was running is restarted and waited for. An agent is restarted by you. See [Upgrading](upgrading.md).

## Uninstall

```sh
sh install.sh --uninstall --yes --json            # keeps configuration, data, skills and accounts
sh install.sh --uninstall --purge --yes --json    # deletes them too
```

`--purge` is destructive and `--yes` stands in for the typed confirmation, so use it only when you mean it. Run `--dry-run` first to see what would go.

## The JSON output

With `--json`, standard output carries only JSON lines. The text goes to the log file (`/var/log/domovoi-install.log`, or `DOMOVOI_INSTALL_LOG`). There is one `step` object per event and one `result` object at the end, also after a failure.

```json
{"event":"step","n":1,"total":6,"name":"preflight","status":"start","message":"Checking this machine"}
{"event":"step","n":1,"total":6,"name":"preflight","status":"ok","message":"Checking this machine"}
```

| Field of a `step` | Meaning |
| --- | --- |
| `n`, `total` | The step's number and the number of steps in this run. |
| `name` | `preflight`, `plan`, `download`, `verify`, `install`, `service`, `start`, `enroll`; for an uninstall `detect`, `plan`, `stop`, `units`, `files`, `purge`. |
| `status` | `start`, `ok`, `warn` or `fail`. |
| `message` | A short sentence. |

The last line is the result:

```json
{"event":"result","ok":true,"exit_code":0,"role":"hub","version":"v0.1.0","layout":"hub","paths":{"binary":"/usr/local/bin/domovoi","log":"/var/log/domovoi-install.log"},"urls":{"web":"http://192.168.1.50:7420"},"admin_created":true,"started":true,"enrolled_host":"","agent_started":false,"next_steps":["..."],"plan":[],"error":"","mode":"install","purge":false,"rolled_back":false,"removed":[],"kept":[]}
```

| Field of the `result` | Meaning |
| --- | --- |
| `ok`, `exit_code` | Success and the exit code (see below). |
| `role`, `version`, `layout` | What was installed. |
| `paths` | The binary and the log file. |
| `urls` | `web`, the address to open, and `setup`, the single-use setup link when no admin account was created. The link is a secret: it is in the JSON result but never in the log. |
| `admin_created` | `true` when `--admin-password-file` created the account `admin`. |
| `started`, `agent_started` | The hub, or the agent, was started in this run. |
| `enrolled_host` | The name the agent was enrolled under. |
| `next_steps` | What is left, as sentences or commands. |
| `plan` | The plan lines; filled on a `--dry-run`. |
| `error` | The reason, when `ok` is `false`. |
| `mode` | `install` or `uninstall`. |
| `purge`, `rolled_back` | An uninstall with `--purge`; a failed fresh install that was undone. |
| `removed`, `kept` | For an uninstall or a rollback, what was deleted and what was left. |

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Done. |
| 2 | Usage: a bad flag or value (including a bad URL, a literal `--token`, an incomplete `--hub`, `--fingerprint` and token set, a password or token file that is not a regular file with mode 0600 or 0400 owned by you, `--purge` without `--uninstall`, or `--purge` with neither `--yes` nor a terminal). |
| 10 | Preflight: this machine cannot take the install, an older version without `--force-downgrade`, or a hub database from before 0.1.0 without `--accept-data-reset`. With `--uninstall`: nothing is installed, a path it would remove is a symbolic link, or it was not run as root. |
| 20 | Download or checksum. |
| 30 | Install: the binary or its files. |
| 40 | Service: users, directories, units or sockets; the hub did not become ready; the admin account could not be created; the agent could not be enrolled or did not connect. With `--uninstall`: a service could not be stopped or an account could not be removed. |
| 50 | The self-test of an agent install failed. |
| 1 | Anything else. |

A fresh install that fails before the first service starts is rolled back: `rolled_back` is `true` and the machine is as it was. After a failure later than that, nothing is undone and `next_steps` says what to run. `--keep-on-failure` leaves a failed fresh install in place.

## What stays with a person

The installer cannot do these, and no flag does:

- Creating the Discord bot and giving Domovoi its token, channel and user id. Do it in the Discord developer portal and under **Settings**; see [Discord](../guide/discord.md).
- Getting an LLM API key or logging in a Claude Code or Codex tool, and entering it under **Settings**; see [LLM providers](../guide/llm-providers.md).
- Approving actions. Only an authenticated human approves what Domovoi proposes, from Discord or the web UI. No flag, endpoint or script approves for you, and an automation agent must not try.
- Installing packages through the Capability Center, which asks for the password and the package name.
