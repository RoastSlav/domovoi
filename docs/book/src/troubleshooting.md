# Troubleshooting

Start with the logs. They name the problem in plain words more often than not.

```sh
sudo journalctl -u domovoi-hub -n 100
sudo journalctl -u domovoi-agent -n 100
docker logs domovoi              # the hub in a container
```

The web UI's **Advisories** list problems the hub found with itself: a failing heartbeat, an exposed secret, an outdated agent, a failed settings change.

## The setup link

**I lost the setup link.** It is printed once at startup while no account exists. Restart the hub and print the latest complete link without a pager: `journalctl -u domovoi-hub --no-pager -o cat | grep '/setup?token=' | tail -n 1`. A link is valid for 24 hours, but restarting replaces its token.

**The link opens the wrong address.** It uses the hub's public URL, which is `http://127.0.0.1:7420` until you set one. For direct LAN access, set both `DOMOVOI_WEB__BIND` and `DOMOVOI_WEB__PUBLIC_URL` as shown in [Binary and systemd](install/binary.md#install-the-hub); merely replacing the hostname in the link causes a cross-origin error. Alternatively, open an SSH tunnel (`ssh -L 7420:127.0.0.1:7420 <host>`) and keep the link unchanged.

**The setup page reports a cross-origin error.** Set `DOMOVOI_WEB__PUBLIC_URL` to the exact origin in the browser, including its scheme and port, then restart the hub and use the newly printed token.

**I forgot the password.** Stop the hub and run `sudo -u domovoi domovoi hub set-password`, then start it again.

## Agents

**The enroll command fails with a connection error.** The agent dials the address in the command, `wss://<name>:7421` until you set the hub's agent address. Set it on **Settings → Hub** to an address the agent can reach, then create a new command with **Add host**. Check that port 7421 is open from the agent to the hub.

**"token is invalid or expired".** Enroll tokens work once, for 15 minutes. Create a new one with **Add host**.

**"certificate fingerprint does not match".** The hub's certificate changed, usually because its data directory was lost or replaced. Restore `tls/` from a backup, or enroll the agent again: remove `/var/lib/domovoi-agent/identity.json`, then run a new enroll command.

**The host shows "silent".** The agent stopped sending heartbeats. Check `systemctl status domovoi-agent` on the host, and the network between the two. If several hosts went silent at once, the hub's own network is the likely cause.

**"revoked" or "unknown agent" in the agent's log.** The host was revoked or re-enrolled elsewhere. Enroll it again.

**"incompatible" in the agent's log.** The agent and the hub speak different protocol `major.minor` versions. Upgrade the hub first, then the agent, and restart the agent. `domovoi agent status` shows both versions.

**A check reports "journalctl is not installed" or "systemctl is not installed".** The agent runs in a container, where those tools are not available. Install the agent on the host for unit and journal checks.

### The hub's own host says Hub only

The host named after the hub is its own machine, shown as "Hub only, no agent" while no agent has ever enrolled there; it is not broken, since the hub records its own problems (like `hub.network_degraded`) against it. An enrolled agent that is merely offline still shows as offline; after a hub restart the hub keeps watching enrolled hosts, so an agent that never reconnects is reported as an `agent.down` incident on that host (see [Host down, agent down, or the hub's own network](guide/agents.md#host-down-agent-down-or-the-hubs-own-network)). If you stopped running the local agent, revoke it with `sudo -u domovoi domovoi hub agent revoke <hub name>` while the hub is stopped, or with **Revoke** on the host's row in the web UI. To also watch that machine's disks, services and containers, enroll a separate agent on it under the hub's name. The hub can instead run the agent inside its own process with `--with-local-agent /etc/domovoi/agent.toml`, but then the two share the hub's unprivileged sandbox, and checks that need root report errors (see [Binary and systemd](install/binary.md)).

## Skills

**A skill is not offered.** Run `sudo domovoi check skills /etc/domovoi/skills`: it explains each problem, such as a group-writable script or a folder name that differs from the manifest's. Then restart the agent; it reads skills when it starts.

**A proposed action is refused when approved.** Look at the action in the web UI. Common reasons: the skill's risk is above the host's `[policy] allow_risk`; the skill or its manifest changed since it was proposed; the proposal expired.

**A skill fails with "permission denied".** On an agent that still runs as root, skills run as root unless the manifest sets `run_as`. On the three-identities layout they run through the skill runner as `domovoi-skill`, which cannot write to its working directory (the skill's own folder, read-only; use `/tmp`), cannot read the agent's or the helper's files and is in no `docker` group. Check also that the program it calls exists in the agent's `PATH` and on the host. If a starter skill such as `restart_container` fails this way, it needs an operation the helper does not offer yet; see [What a skill can reach](guide/skills.md#what-a-skill-can-reach).

## Identities and the helper

Run `sudo domovoi agent selftest --installer` to see every check. [Command line](reference/cli.md#domovoi-agent-selftest) lists them with their tokens.

### The host says "privilege boundary not in effect"

The agent is not running as the `domovoi-agent` account with its expected groups. Common causes: an agent installed before the three-identities layout (it runs as root), a hub on the same host, a container install (the host is also labelled a container agent, and write capabilities are refused with `agent_in_container`), or no systemd. For an old root agent, `sudo sh install.sh --role agent --migrate-identity` moves it ([Binary and systemd](install/binary.md#already-installed)). The host gets no unattended execution until the boundary is in effect.

### The helper refuses to start

The helper's own checks failed, and it answers every request with `StartRefused` and the list of paths. The host's helper status on the hub shows the token. Read the exact paths in the journal: `sudo journalctl -u 'domovoi-helper@*' -n 50`.

- `helper_state_permissions`: a mode, owner, group or type is wrong on the helper's directory, lock, key, state, platform record or `helper.toml`, or one of them is a symlink. Each path is listed with the found and the expected values. Nothing was changed. Fix it with `sudo domovoi helper repair`, which shows the plan and asks you to type `repair`. If a path is a symlink or the wrong type, remove or fix it yourself first: repair refuses those.
- `uninitialized`: `domovoi helper init` has not run. Run it with `sudo`.
- `helper_config_invalid`: `/etc/domovoi/helper.toml` has an unknown key, an unknown group or a duplicate group. Fix the file.
- `agent_account_missing`: the `domovoi-agent` account does not exist, so the helper does not know whom to serve.

### A path has the wrong mode or owner

`path_mode_wrong` (check ST-10) names every path whose mode, owner, group or type differs from the layout, or whose skills or packages tree has a group or other write bit. The installer reports the same and stops before it enables the helper socket. Neither changes anything. Run `sudo domovoi helper repair` to apply the listed changes after you confirm; it covers modes and owners only, never file content.

### `unit_modified`

Check ST-07 found a drop-in on the helper socket, the helper service, the skill runner socket or service, or the agent unit, or a unit change that has not been loaded, or a runner unit whose directories (`bind_paths`) no longer match `agent.toml`. Look for the file it lists (usually under `/etc/systemd/system/<unit>.d/`), remove it, run `sudo systemctl daemon-reload` and run the check again. `unit_not_inspectable` means `systemctl show` failed for that unit; check that it is installed. A drop-in can weaken the isolation the units set, so the check cannot accept it, and `systemctl edit` on these units will keep skills blocked.

### The helper answers a read with a token

A check or tool that reads through the helper shows the helper's token. Each one means:

| Token | What to do |
| --- | --- |
| `tool_missing` | Install `smartmontools` (`smartctl`), `util-linux` (`dmesg`) or `iproute2` (`ss`). The helper looks only at fixed paths such as `/usr/sbin/smartctl`, and only accepts a root-owned file that others cannot write. |
| `engine_absent` | No Docker or Podman socket at `/var/run/docker.sock` or `/run/podman/podman.sock`. Set `[docker] endpoint = "unix:///your/socket"` in `/etc/domovoi/helper.toml`, or ignore it on a host without containers. A rootless Podman socket cannot be read by the helper. |
| `engine_denied` | The socket exists but the helper may not open it. Check the socket's owner and any security module (SELinux, AppArmor) that confines the helper. |
| `engine_api_refused` | The engine or a proxy in front of it answered 403 or 405 to a read. Allow `GET` on containers, images, events and logs. |
| `device_not_allowed` | The device is not in `[smart] devices` in `helper.toml`. Add a pattern such as `/dev/sd[a-z]` and restart nothing: the helper reads the file at every request. |
| `command_failed` | The tool failed; the detail shows the first line of its error. `Operation not permitted` or `Permission denied` on an already migrated host usually means the helper unit is the old one; see [Upgrading](install/upgrading.md). |
| `group_disabled` (shown as "switched off") | The group is not in `groups` in `helper.toml`. Add it back to use that read. |

### Skills are unavailable: the skill runner

On the three-identities layout skills run only through the skill runner, one process per connection to `/run/domovoi-skill-runner.sock`. The host's skill runner status on the hub, and the agent's log, show one of these:

| Status | Reason | What it means and what to do |
| --- | --- | --- |
| `Absent` | none | The socket does not exist. The installer enables `domovoi-skill-runner.socket` only after the self-test passes; check `systemctl status domovoi-skill-runner.socket` and run `sudo domovoi agent selftest --installer`. |
| `Unsupported` | `runner_groups_extra` | The `domovoi-skill` account has a group besides its own. Remove it (`sudo gpasswd -d domovoi-skill <group>`). Nothing was changed for you. |
| `Unsupported` | `runner_privileges` | The runner process has a capability or can gain privileges, usually because a drop-in changed its unit. Remove the drop-in; see `unit_modified`. |
| `Error` | `runner_socket_permission` | The agent cannot connect to the socket. It should be `root:domovoi-agent`, mode 0660. Check ST-12 and the agent's groups. |
| `Error` | `runner_protocol_mismatch` | The agent and the runner binary are different versions, usually after an upgrade. Restart the agent: `sudo systemctl restart domovoi-agent`. |
| `Error` | `runner_roots_differ` | `skills_dir` or `packages_dir` in `agent.toml` is not the directory the runner unit was rendered with. Run the installer again to re-render the unit, then restart the agent. |
| `Error` | `runner_unreachable` | The socket exists but no answer came: the runner crashed, or its unit failed to start. See below. |

Read the runner's own messages with `sudo journalctl -u 'domovoi-skill-runner@*' -n 50`. A start that fails with `226/NAMESPACE` means a directory the unit binds does not exist (`skills_dir` or `packages_dir`); `ST-10` names it. A crashed runner leaves a failed instance behind. Clear it with:

```sh
sudo systemctl reset-failed 'domovoi-skill-runner@*'
```

**A skill is refused with `run_as_unsupported`.** The failure reads `run_as_unsupported: <skill> names run_as <user>; only domovoi-skill is supported on this host`. A migrated host runs skills only as `domovoi-skill`. Remove the `run_as` line from the manifest (or set it to `domovoi-skill`), run `domovoi check skills`, and restart the agent. `domovoi agent selftest` lists every affected skill. The hub shows the skill as failed with this message; it does not yet show the reason separately.

**Skills stopped working after I edited `agent.toml` or a unit.** The runner gets its two directories from its unit, not from `agent.toml`. Run the installer again to re-render it. `unit_modified` with `bind_paths` means the unit and the file disagree. A drop-in that removes the unit's mount mask makes the runner fail closed, so skills stop; remove it.

## Discord

**Nothing is posted.** Run `sudo -u domovoi domovoi notify test`. It reports PASS or FAIL for each gateway with the reason. Check the bot was invited with the permissions in the [Discord guide](guide/discord.md), and the channel id is right.

**Buttons answer "Not allowed".** Your Discord user id is not in the allowlist. Add it on **Settings → Discord**.

**The bot ignores questions in threads.** Turn on **Message Content Intent** for the bot in the Discord developer portal.

## Cards

**Cards stopped appearing for one incident.** The hub could not read that incident's notification state, so it stopped delivering for that incident rather than posting a duplicate card. The log says `could not read notification state`, and `domovoi_notify_state_errors_total{op="read"}` rises. Its `notify_state` row is corrupt: with the hub stopped, restore it from a backup or delete the row.

## Triage

**Cards have no verdict.** Triage failed or is off, or `[orchestration] enabled` is `false`, which stops automatic investigation (it is on by default). The card still goes out, so you never miss an alert. Use **Test** on **Settings → Models and triage**, and look in the hub's log for the provider's error.

**Triage stopped for the month.** The monthly budget is spent. Raise it on **Settings → Models and triage**, or wait for the next month.

**A `cli` provider fails under systemd.** The CLI runs as the `domovoi` user and needs its own login: `sudo -u domovoi -H claude login`. Claude Code also needs `MemoryDenyWriteExecute=no`; see [Binary and systemd](install/binary.md#the-units).

## Advisories

**The advisory says a background task failed permanently.** It is kind `effect_dead`: an effect, such as the re-check after an action, used up its attempts and stopped. The advisory's detail names the effect, its aggregate and the last error, and the hub's log carries the same failures. If it was a verification re-check, the action is recorded as verification failed: check the host and the agent, then run the check by hand or run triage again.

## Still stuck

Search the [issues](https://github.com/RoastSlav/domovoi/issues), or open one with the version (`domovoi version`), what you did, and the relevant log lines. Remove secrets from logs before you paste them.
