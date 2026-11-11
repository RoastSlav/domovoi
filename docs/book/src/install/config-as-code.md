# Advanced: config as code

A hub needs no configuration file. Everything except three bootstrap settings can be set in the web UI, and most installs never need anything else. If you manage machines as code, you can still drive every setting from `hub.toml` and environment variables. Values you set that way win over the UI, and the UI shows them read-only.

## Where a setting comes from

Each setting comes from the highest layer that sets it:

1. Built-in defaults.
2. Values saved in the web UI.
3. `hub.toml`.
4. `DOMOVOI_*` environment variables.

The Settings page tags every field with where its value comes from: "from hub.toml", "from environment", "set here" or "default". A field set in the file or the environment is read-only there and says so ("Set in hub.toml; edit the file to change it."); change it where it is set.

Settings are grouped into sections, listed on the left, and each has its own address (`/settings/discord`). A section with unsaved changes is marked in the list, and so is one the hub found a problem in. One **Save** at the bottom saves every section's changes together and restarts the hub; the page shows "Restarting the hub…" and then "Back online". If a value is not accepted, the problem is shown under its field and listed at the top, where choosing it takes you to the field. A saved token or key is never shown: its field says "Set" or "Not set", and **Replace** lets you enter a new one. Sections that talk to something outside the hub (Discord, the model, webhooks, the heartbeat) have a **Test** button that tries the connection with the values on the page.

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
