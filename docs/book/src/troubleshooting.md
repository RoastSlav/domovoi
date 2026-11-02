# Troubleshooting

Start with the logs. They name the problem in plain words more often than not.

```sh
sudo journalctl -u domovoi-hub -n 100
sudo journalctl -u domovoi-agent -n 100
docker logs domovoi              # the hub in a container
```

The web UI's **Advisories** list problems the hub found with itself: a failing heartbeat, an exposed secret, an outdated agent, a failed settings change.

## The setup link

**I lost the setup link.** It is printed once at startup while no account exists. Restart the hub and search the log again: `sudo journalctl -u domovoi-hub | grep -A1 setup`. A link is valid for 24 hours.

**The link opens the wrong address.** It uses the hub's public URL, which is `http://127.0.0.1:7420` until you set one. Replace the host part with the hub machine's address, or open an SSH tunnel: `ssh -L 7420:127.0.0.1:7420 <host>`.

**I forgot the password.** Stop the hub and run `sudo -u domovoi domovoi hub set-password`, then start it again.

## Agents

**The enroll command fails with a connection error.** The agent dials the address in the command, `wss://<name>:7421` until you set the hub's agent address. Set it on **Settings → Hub** to an address the agent can reach, then create a new command with **Add host**. Check that port 7421 is open from the agent to the hub.

**"token is invalid or expired".** Enroll tokens work once, for 15 minutes. Create a new one with **Add host**.

**"certificate fingerprint does not match".** The hub's certificate changed, usually because its data directory was lost or replaced. Restore `tls/` from a backup, or enroll the agent again: remove `/var/lib/domovoi-agent/identity.json`, then run a new enroll command.

**The host shows "silent".** The agent stopped sending heartbeats. Check `systemctl status domovoi-agent` on the host, and the network between the two. If several hosts went silent at once, the hub's own network is the likely cause.

**"revoked" or "unknown agent" in the agent's log.** The host was revoked or re-enrolled elsewhere. Enroll it again.

**"incompatible" in the agent's log.** The agent is newer than the hub, or more than one minor version behind it. Upgrade the hub first, then the agent.

**A check reports "journalctl is not installed" or "systemctl is not installed".** The agent runs in a container, where those tools are not available. Install the agent on the host for unit and journal checks.

## Skills

**A skill is not offered.** Run `sudo domovoi check skills /etc/domovoi/skills`: it explains each problem, such as a group-writable script or a folder name that differs from the manifest's. Then restart the agent; it reads skills when it starts.

**A proposed action is refused when approved.** Look at the action in the web UI. Common reasons: the skill's risk is above the host's `[policy] allow_risk`; the skill or its manifest changed since it was proposed; the proposal expired.

**A skill fails with "permission denied".** Skills run as the agent's user (root with the packaged unit) unless the manifest sets `run_as`. Check the program it calls exists in the agent's `PATH` and on the host.

## Discord

**Nothing is posted.** Run `sudo -u domovoi domovoi notify test`. It reports PASS or FAIL for each gateway with the reason. Check the bot was invited with the permissions in the [Discord guide](guide/discord.md), and the channel id is right.

**Buttons answer "Not allowed".** Your Discord user id is not in the allowlist. Add it on **Settings → Discord**.

**The bot ignores questions in threads.** Turn on **Message Content Intent** for the bot in the Discord developer portal.

## Triage

**Cards have no verdict.** Triage failed or is off. The card still goes out, so you never miss an alert. Use **Test** on **Settings → Models and triage**, and look in the hub's log for the provider's error.

**Triage stopped for the month.** The monthly budget is spent. Raise it on **Settings → Models and triage**, or wait for the next month.

**A `cli` provider fails under systemd.** The CLI runs as the `domovoi` user and needs its own login: `sudo -u domovoi -H claude login`. Claude Code also needs `MemoryDenyWriteExecute=no`; see [Binary and systemd](install/binary.md#the-units).

## Still stuck

Search the [issues](https://github.com/RoastSlav/domovoi/issues), or open one with the version (`domovoi version`), what you did, and the relevant log lines. Remove secrets from logs before you paste them.
