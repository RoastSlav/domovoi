# 0018. Detect the container runtime and let the hub switch it

- Status: Accepted
- Date: 2026-11-07

## Context

The agent reads containers through the Docker Engine API for four checks and four read tools. Until now that was off unless `agent.toml` said `[docker] enabled = true`, and the only socket it knew was `/var/run/docker.sock`. Someone who installed the agent on a host with Docker saw "Docker is not enabled on this host" from the triage tools, and had to find the setting in a file on that host.

Access to a container engine's socket is root on the host (`guide/hardening.md`). That is why the integration was opt-in. But the agent only ever reads: it lists containers, reads events, inspects and reads logs, and removes environment values from inspect output. Podman and rootless Docker answer the same API on other sockets, so a host running them was never covered.

ADR 0017 already moves check enablement to the web UI and says `agent.toml` keeps only what the agent needs to reach the hub and read the host.

## Decision

### The agent looks for an engine on every start

- `[docker] enabled` takes `"auto"` (the default), `true` or `false`. `[docker] endpoint` is optional.
- With `"auto"` and no endpoint, the agent tries these sockets in order and uses the first that accepts a connection: `/var/run/docker.sock`, `$XDG_RUNTIME_DIR/docker.sock` (rootless Docker), `/run/podman/podman.sock`, `$XDG_RUNTIME_DIR/podman/podman.sock` (rootless Podman). With an endpoint, only that one is tried; a `tcp://` endpoint is tried too.
- Detection runs at every start and its result is never stored, so an engine installed later is picked up by restarting the agent. It opens and closes a connection and sends nothing.
- A socket that exists but refuses the agent's user is reported with that reason instead of "not found", so the fix (a group) is visible.
- Podman needs its API socket running (`systemctl enable --now podman.socket`). Other engines that answer the Docker Engine API work by setting `endpoint`.

### Who decides

- `true` and `false` stay what they were: the host owner's word, kept in a file the hub cannot write. The hub cannot turn a `false` into `true`, and cannot turn a `true` off.
- With `"auto"`, the integration is on when an engine was found, and the hub may switch it off or back on for that host. The hub can only switch on an engine the agent found. It never makes the agent look at another address, so it cannot point the agent at a socket the owner did not expect.
- The choice is a per-host value in the `settings` table (`host.<name>.docker`), sent in `welcome` and `config_update` like check overrides, and applied live. While the integration is off, the four container checks do not run and the container tools answer "switched off for this host".

### Protocol

Two optional fields, so the protocol stays at 1.0 and old agents and hubs keep working:

- `hello.docker`: what the agent found (`engine`, `endpoint`, `available`, `locked`, `enabled`, `problem`).
- `welcome.config.docker` and `config_update.config.docker`: the hub's choice, absent when the agent decides.

### Web API

`GET` and `PUT /api/v1/hosts/{name}/docker`. The `PUT` needs an admin session and the CSRF token, writes a `settings.host_docker` audit event, and is refused when the agent reported that its file fixes the setting or that no engine was found. An agent that has not reported (offline or too old) is not refused; the choice waits for it.

## Consequences

- A host with Docker or Podman gets container checks and tools without editing a file. Hosts without an engine log one line and carry on.
- Enabling by default widens what a new install reads on the host. It is read-only, the output is redacted, and `enabled = false` turns it off for good. Release notes must say so.
- The agent's user needs access to the socket. The docs say how for Docker and Podman.
- Containers still never get the socket itself: the container agent keeps reading through the read-only socket proxy and sets `endpoint`.
- The hub's choice is not a secret and not an executable change. No LLM tool, chat command, webhook or agent message can write it (INV-3 and the settings gates are unchanged).

## Alternatives considered

- Keep it off by default with a better error message: still a file edit on the host before anything works.
- Let the hub also set the endpoint: it would let the web UI point the agent at any socket on the host.
- Re-detect while running: simpler to reason about when the agent decides once per start; an engine that appears later needs one restart.
