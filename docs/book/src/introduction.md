# Introduction

> **Pre-alpha status:** The core runtime libraries and web UI are under development. The
> `domovoi` binary currently provides `version` and a hidden OpenAPI command; it does not
> yet start a production hub or agent. There are no release binaries, container images,
> or supported upgrade path. Follow [Quick start](quick-start.md) to preview the UI with
> fake hosts and incidents.

Domovoi is a self-hosted overseer for your servers. It watches hosts with built-in checks and the alerts you already have, groups what it sees into incidents, asks an LLM of your choice to triage each one with read-only tools, and talks to you in a Discord thread per incident. It never changes anything on a host unless you approve the action yourself.

It is named after the domovoi, the Slavic household spirit who quietly keeps a home in order.

## Who it is for

- Homelab and small self-hosting setups: one to a handful of Linux machines running Docker and systemd services.
- People who currently glue together cron scripts, a chat webhook and maybe an LLM call, and want one tool that does it properly and safely.

## What it is not

- Not a metrics database or dashboard. Keep Grafana, Netdata or Beszel for graphs; Domovoi links to them and exposes its own `/metrics`.
- Not an autonomous agent that fixes things on its own. The model proposes; you approve.

The guides describe the intended user workflows and the runtime components currently
implemented in the libraries. Pages about installation and operations explain which
parts are still planned.
