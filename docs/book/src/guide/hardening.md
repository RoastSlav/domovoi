# Hardening

Domovoi's defaults are safe on a home network. Before you expose the web UI beyond it, or add hosts you care about more, go through this page. The [security model](../security.md) explains what each part protects against.

## The web UI

- **Keep the listener on loopback** (the default, `127.0.0.1:7420`) and publish it through a reverse proxy that terminates TLS: Caddy, nginx or Traefik.
- **Set the public URL** to the proxy's `https://` address, in **Settings → Hub**. Sessions then get the `Secure` cookie flag, and requests from any other origin are refused.
- **List the proxy in `[web] trusted_proxies`**, so login rate limits apply to real client addresses instead of the proxy's.
- **Use a long password.** After five failed logins in a minute an address is locked out, and each further lockout doubles, up to 15 minutes.
- Do not put the UI on the open internet without something in front of it, such as a VPN, Tailscale, or your proxy's single sign-on.

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
- **Keep skills owned by root and not writable by others.** The agent refuses group- or world-writable entries, and folders that anyone can write to.

## Docker

- **Never mount `docker.sock` into a container**, the agent's included: access to the socket is root on the host. The [container agent](../install/container-agent.md) reads Docker through a socket proxy that only allows reads.
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

The packaged hub unit runs as the unprivileged `domovoi` user with a read-only system, no access to home directories and no new privileges. Keep changes to drop-ins (`systemctl edit domovoi-hub`), so upgrades keep the sandbox. The agent runs as root, because SMART, the journal and skills need it; its unit still keeps `/usr` and `/etc` read-only. See [Binary and systemd](../install/binary.md#the-units).
