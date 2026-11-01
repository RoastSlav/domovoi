# Hub and agents

Domovoi is one binary with two roles.

The **hub** holds everything that is shared: the database of hosts, incidents and actions, the web UI and API, chat, triage, approvals, the digest, and network probes. You run one.

An **agent** runs on each machine you want to watch. It runs the built-in checks on its own schedule, relays local webhooks, answers the read-only questions triage asks, and runs the skills installed on it after a person approves them. It keeps no state the hub needs, so it can be reinstalled at any time.

## The connection

Agents connect out to the hub over WebSocket and TLS, on port 7421. Nothing connects to an agent, so a host needs no open port.

- When it enrolls, an agent pins the hub's certificate fingerprint and registers a key of its own. Every connection after that is mutually authenticated: the agent checks the certificate, and the hub challenges the agent to sign with its key.
- An agent sends a heartbeat every 10 seconds. After three missed heartbeats the hub marks it silent, first checking its own network (`[agent_server] self_check`) so a hub-side outage is not blamed on every host.
- While disconnected, an agent spools signals on disk and sends them when it is back.

The [agent protocol](../reference/protocol.md) has the details.

## Versions

A hub accepts agents on its own protocol version or one minor version behind; the Hosts page marks agents older than the hub. Upgrade the hub first, then the agents. See [Upgrading](../install/upgrading.md).

## Where to run the hub

- **On one machine with an agent**: the simplest start. Install both roles, as in the [quick start](../quick-start.md). If that machine dies, nobody notices.
- **On a separate small machine**: a Raspberry Pi or a VM elsewhere. Now a host that goes silent is noticed and reported.
- **In a container**: see [Docker](../install/docker.md). Run the [container agent](../install/container-agent.md) next to it to watch the Docker host.

The hub can also run an agent for its own machine inside its own process (`domovoi hub run --with-local-agent agent.toml`). The two then share the hub's unprivileged sandbox, so checks that need root report errors. Two separate services are usually the better choice; see [one machine](../install/binary.md#one-machine).
