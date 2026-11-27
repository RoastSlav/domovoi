# Hardening

Domovoi's defaults are safe on a home network. Before you expose the web UI beyond it, or add hosts you care about more, go through this page. The [security model](../security.md) explains what each part protects against.

## The web UI

- **Keep the listener on loopback** (the default, `127.0.0.1:7420`) and publish it through a reverse proxy that terminates TLS: Caddy, nginx or Traefik.
- **Set the public URL** to the proxy's `https://` address, in **Settings → Hub**. Sessions then get the `Secure` cookie flag, and requests from any other origin are refused.
- **List the proxy in `[web] trusted_proxies`**, so login rate limits apply to real client addresses instead of the proxy's.
- **Use a long password.** After five failed logins in a minute an address is locked out, and each further lockout doubles, up to 15 minutes. The same limit applies per username (case-insensitive), so guesses spread over many addresses still lock the account for the same time; the price is that someone who keeps failing as your username can lock you out of it too, until the lockout ends. Both lockouts are kept in memory (a restart clears them). Behind Docker's userland proxy, or behind a reverse proxy that is not listed in `[web] trusted_proxies`, every client has the same address (often `172.17.0.1`), so one bad actor locks everybody out. Publish the port without the userland proxy (`"userland-proxy": false` in Docker's `daemon.json`) or list your proxy in `trusted_proxies` so the real client address is used.
- Do not put the UI on the open internet without something in front of it, such as a VPN, Tailscale, or your proxy's single sign-on.

## Settings that need the password again

A stolen session is not enough to widen what the hub trusts. Saving a change to any of these asks for the admin's password again, even while logged in:

| Setting | Why |
| --- | --- |
| `discord.allowed_user_ids` | Widens who can approve actions from chat. |
| `web.trusted_proxies` | Changes which client addresses the hub trusts. |
| `webhooks` and anything under it | Webhook secrets and sources decide who can raise incidents. |
| `llm.<slot>.cli` (preset, command, args) | The program, and its arguments, the hub runs as the model. A stolen session could otherwise point it at any binary. |
| `metrics` and anything under it | Who can read the hub's metrics, including the scraper token. |
| `retention` and anything under it | How long data is kept. |
| Any secret, set or cleared | Bot tokens, API keys, webhook secrets, heartbeat URLs and the metrics token. |

A wrong password shares the login rate limit, so repeated attempts lock the address out as logins do. Publishing a policy version, enabling a capability package, starting an installation and turning autonomy back on will ask for the password too, once those features exist.

## Reading metrics

`GET /metrics` needs a web session or a bearer token from `[metrics] token`; it is never open to anyone who can reach the web port. A Prometheus scraper needs the token; see the [CLI reference](../reference/cli.md#metrics).

A minimal Caddy site:

```text
domovoi.example.net {
    reverse_proxy 127.0.0.1:7420
}
```

## Agents

- **Agents dial out**, so open port 7421 only towards the hub, and only from your network. Nothing connects to an agent.
- **Agents pin the hub's certificate** at enrollment, and the hub knows each agent by its key. Keep the hub's `tls/` directory in your backups; losing it means enrolling every agent again.
- **Revoke** a host you retire, on **Hosts**. Its key is then refused.
- **Limit risk per host** with `[policy] allow_risk` in `agent.toml`. The default allows `read` and `write` skills. Add `destructive` only where you want such skills to be possible at all.
- **Run the agent as `domovoi-agent`.** A fresh install on a systemd host with no hub does.
- **Keep skills owned by root and not writable by others.** The agent refuses group- or world-writable entries, and folders that anyone can write to.

## Docker

- **Never mount `docker.sock` into a container**, the agent's included: access to the socket is root on the host. The [container agent](../install/container-agent.md) reads Docker through a socket proxy that only allows reads. It gets no helper and no skill runner, so it cannot change the host; the hub refuses those capabilities with `agent_in_container`.
- **Do not add `domovoi-agent` to the `docker` group.** On the three-identities layout the root helper reads the socket for the agent, through five read-only calls ([Read operations](../reference/helper.md#read-operations)). If you want the agent to use a socket proxy instead, set a `tcp://` `endpoint` in `agent.toml`; that connection stays direct.
- **The helper unit is the widest of the three.** To read disks, the kernel log and other users' sockets it keeps six capabilities and read access to disk devices; see [Helper](../reference/helper.md#the-units). Remove a read group from `helper.toml` if you do not want it used, knowing the unit's capabilities stay.
- Allow `POST` on the proxy only if you use container skills that change containers, and keep the proxy on a network only the agent shares.

## Secrets

- **Never write a secret inline** in `hub.toml` or `agent.toml`: use `{ env = "…" }` or `{ file = "…" }`, with the file readable only by its owner. Secrets entered in the web UI are encrypted with `secret.key` in the hub's data directory.
- **Back up `secret.key` apart from the database.** See [Backups](../reference/backup.md).
- **If a secret leaks**, rotate it at its source, then list its name under `[secrets] exposed`. The hub raises an advisory until you remove it.
- Secrets are redacted from logs, model requests, chat and the API. Triage still reads logs, so keep secrets out of log lines in the first place where you can.

## The model

- The model only gets read-only tools, and its output is never run. Even so, everything triage reads is sent to your provider. Use a local model through an OpenAI-compatible server if that matters to you.
- Set a monthly budget in **Settings → Models and triage**; triage pauses when it is reached.

## Discord

- **Only allowlisted user ids** can press buttons or talk to the bot; everyone else gets "Not allowed".
- Turn off **Public Bot** in the developer portal, so nobody else can add it to a server.
- Destructive actions never get an approve button in chat, whoever presses it.

## The service

The packaged hub unit runs as the unprivileged `domovoi` user with a read-only system, no access to home directories and no new privileges. A fresh agent install runs as the unprivileged `domovoi-agent` user, with the root helper behind its own socket (see [Security model](../security.md#three-identities-on-one-host)); an agent on a host with a hub runs as root. The agent unit keeps `/usr` and `/etc` read-only either way. See [Binary and systemd](../install/binary.md#the-units).

### Units and drop-ins

Keep changes to the hub unit in drop-ins (`systemctl edit domovoi-hub`), so upgrades keep the sandbox. Do not put a drop-in on the helper units (`domovoi-helper.socket`, `domovoi-helper@.service`) or on `domovoi-agent.service`, and do not run `systemctl edit` on them. `domovoi agent selftest` reads each unit's effective properties and reports any drop-in, and any change waiting for a `daemon-reload`, as `unit_modified`, with the platform then blocked for skills. A drop-in can quietly weaken the user, the groups or the socket mode these units set, and the check cannot tell a harmless one from a harmful one. Change the shipped unit text through an upgrade instead, or leave it as it is.

Also keep these as installed:

- the helper socket at `root:domovoi-agent`, mode 0660;
- `/etc/domovoi/skills` and `/var/lib/domovoi-packages` with no group or other write bit;
- no Domovoi account in the `docker` group.
