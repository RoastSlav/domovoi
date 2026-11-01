# Advanced: config as code

A hub needs no configuration file. Everything except three bootstrap settings can be set in the web UI, and most installs never need anything else. If you manage machines as code, you can still drive every setting from `hub.toml` and environment variables. Values you set that way win over the UI, and the UI shows them read-only.

## Where a setting comes from

Each setting comes from the highest layer that sets it:

1. Built-in defaults.
2. Values saved in the web UI.
3. `hub.toml`.
4. `DOMOVOI_*` environment variables.

The Settings page labels a field set in the file "Set in hub.toml", and one set in the environment "Set by `DOMOVOI_…`". Those fields are read-only there; change them where they are set.

`domovoi hub run` reads `--config <file>`, or `DOMOVOI_HUB_CONFIG`, or `/etc/domovoi/hub.toml` when that file exists. Without any of them the hub runs on defaults, the environment and the UI.

## Bootstrap settings

These are needed before the hub can open its database, or could lock you out of the UI if they were wrong. They are only read from the file or the environment, never from the UI:

| Setting | Variable | Default |
| --- | --- | --- |
| `hub.data_dir` | `DOMOVOI_HUB__DATA_DIR` | `/var/lib/domovoi` |
| `web.bind` | `DOMOVOI_WEB__BIND` | `127.0.0.1:7420` |
| `agent_server.bind` | `DOMOVOI_AGENT_SERVER__BIND` | `0.0.0.0:7421` |

## Environment variables

Any scalar setting can be set with `DOMOVOI_<SECTION>__<KEY>`, using a double underscore between levels. For example, `DOMOVOI_NOTIFY__DIGEST__TIME=08:00` sets `[notify.digest] time`. Lists and tables need the file. In single-box mode the hub and the local agent share one environment, and each ignores the other's sections.

## Secrets

In `hub.toml` a secret is never written inline. It names where the value lives:

```toml
[discord]
bot_token = { env = "DISCORD_BOT_TOKEN" }
webhook_url = { file = "/run/secrets/discord_webhook" }
```

Secrets entered in the web UI are encrypted in the database with the key in `<data_dir>/secret.key`. See [Backups](../reference/backup.md) for why that key needs its own backup.

## Checking a file

```sh
domovoi check config /etc/domovoi/hub.toml
```

This reports every problem with its path, for example `error: notify.digest.time: …`. `examples/hub.toml` is a short commented starting point; `examples/hub.full.toml` shows every option with its default.

## Agents

`domovoi agent enroll` writes `agent.toml` when it does not exist. It holds only what the agent needs to reach the hub and read the host: its name, the hub's address and its directories. Check settings (enabled, interval, thresholds) are managed per host on the host's Checks tab. They are sent to the agent live, on top of anything `agent.toml` sets. Skill manifests stay files on the agent's host; the hub never sends executable content to an agent.
