# 0003. Hub and agent roles; agent dials out over mutually authenticated TLS

- Status: Accepted
- Date: 2026-10-31

## Context

A monitor on the host it guards dies with that host. Users want the watcher on another machine, without opening inbound ports on the guarded host, and still need local execution of checks and skills.

## Decision

- One binary with two roles. The **hub** owns state, UI, chat, triage and approvals. The **agent** runs on each guarded host and owns checks, skills and read tools.
- The agent connects out to the hub over a TLS WebSocket. The hub uses a self-signed certificate that the agent pins at enrollment.
- Enrollment uses a single-use, 15-minute token. The agent generates an ed25519 key pair; every connection proves possession of the key by signing a hub challenge.
- The hub never executes commands. Single-box mode embeds an agent in the hub process over an in-memory link that carries the same protocol messages.

## Consequences

- Host death, agent death and network loss become distinguishable from the hub.
- The protocol is a public contract: snapshot-tested, versioned (major, minor), with a one-minor compatibility window.
- Agents need their own spool for signals while disconnected.

## Alternatives considered

- Agentless SSH from the hub: needs inbound SSH and broad credentials on every host, and turns the hub into a shell-capable target.
- Hub pulls from agents over HTTP: needs inbound ports on guarded hosts.
