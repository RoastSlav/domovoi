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

**A skill fails with "permission denied".** On an agent that still runs as root, skills run as root unless the manifest sets `run_as`; on the three-identities layout they wait for the skill runner and run as `domovoi-skill`. Check the program it calls exists in the agent's `PATH` and on the host.

## Identities and the helper

Run `sudo domovoi agent selftest --installer` to see every check. [Command line](reference/cli.md#domovoi-agent-selftest) lists them with their tokens.

### The host says "privilege boundary not in effect"

The agent is not running as the `domovoi-agent` account with its expected groups. Common causes: an agent installed before the three-identities layout (it runs as root), a hub on the same host, a container install, or no systemd. For an old root agent, `sudo sh install.sh --role agent --migrate-identity` moves it ([Binary and systemd](install/binary.md#already-installed)). The host gets no unattended execution until the boundary is in effect.

### The helper refuses to start

The helper's own checks failed, and it answers every request with `StartRefused` and the list of paths. The host's helper status on the hub shows the token. Read the exact paths in the journal: `sudo journalctl -u 'domovoi-helper@*' -n 50`.

- `helper_state_permissions`: a mode, owner, group or type is wrong on the helper's directory, lock, key, state, platform record or `helper.toml`, or one of them is a symlink. Each path is listed with the found and the expected values. Nothing was changed. Fix it with `sudo domovoi helper repair`, which shows the plan and asks you to type `repair`. If a path is a symlink or the wrong type, remove or fix it yourself first: repair refuses those.
- `uninitialized`: `domovoi helper init` has not run. Run it with `sudo`.
- `helper_config_invalid`: `/etc/domovoi/helper.toml` has an unknown key, an unknown group or a duplicate group. Fix the file.
- `agent_account_missing`: the `domovoi-agent` account does not exist, so the helper does not know whom to serve.

### A path has the wrong mode or owner

`path_mode_wrong` (check ST-10) names every path whose mode, owner, group or type differs from the layout, or whose skills or packages tree has a group or other write bit. The installer reports the same and stops before it enables the helper socket. Neither changes anything. Run `sudo domovoi helper repair` to apply the listed changes after you confirm; it covers modes and owners only, never file content.

### `unit_modified`

Check ST-07 found a drop-in on the helper socket, the helper service or the agent unit, or a unit change that has not been loaded. Look for the file it lists (usually under `/etc/systemd/system/<unit>.d/`), remove it, run `sudo systemctl daemon-reload` and run the check again. `unit_not_inspectable` means `systemctl show` failed for that unit; check that it is installed. A drop-in can weaken the isolation the units set, so the check cannot accept it, and `systemctl edit` on these units will keep skills blocked once the skill runner ships.

### Skills are unavailable on a new install

On the three-identities layout skills run only through the skill runner, which is not installed yet, so the host is Blocked for skills. That is expected and not a fault.

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
