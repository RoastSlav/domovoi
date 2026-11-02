# Introduction

Domovoi is a self-hosted overseer for your servers. It watches hosts with built-in checks and the alerts you already have, groups what it sees into incidents, asks an LLM of your choice to triage each one with read-only tools, and talks to you in a Discord thread per incident. It never changes anything on a host unless you approve the action yourself.

It is named after the domovoi, the Slavic household spirit who quietly keeps a home in order.

## Who it is for

- Homelab and small self-hosting setups: one to a handful of Linux machines running Docker and systemd services.
- People who currently glue together cron scripts, a chat webhook and maybe an LLM call, and want one tool that does it properly and safely.

## What it is not

- Not a metrics database or dashboard. Keep Grafana, Netdata or Beszel for graphs; Domovoi links to them and exposes its own `/metrics`.
- Not an autonomous agent that fixes things on its own. The model proposes; you approve.

## When to use it

Domovoi fits when you run a few machines yourself and want to hear about problems that matter, with an explanation and a safe way to fix the common ones, without watching dashboards. It is a good fit if:

- you already have alerts, but each one means logging in to find out what happened;
- you have cron scripts that check things and post to a chat, and want them to keep state and stop repeating themselves;
- you want an LLM's help with incidents, but never want it to run commands on your servers by itself.

It is not a good fit for large fleets, compliance auditing, or when you need long-term metrics and graphs.

## Where to start

- [Quick start](quick-start.md): one machine, about ten minutes.
- [How it works](concepts/how-it-works.md): from a check to a card to an approved action.
- [Security model](security.md): what the model can and cannot do.
