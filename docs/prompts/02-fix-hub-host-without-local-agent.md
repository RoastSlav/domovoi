# 02 · Fix: the hub's own host shows "Never connected" when no local agent runs

## Goal

When the hub runs without `--with-local-agent`, its own host entry must not look like a
broken agent. Show it as "Hub only, no agent" with a neutral colour and explain how to
watch that machine.

## What is already known (verified)

- `crates/domovoi-hub/src/runtime/adapters.rs`, `ensure_hub_host()` always creates a host
  named after `hub.name` with `kind: HostKind::Local`. The hub needs it: it raises its own
  signals against it (for example `hub.network_degraded` in
  `crates/domovoi-hub/src/agent_server/presence.rs`). Only this function creates `Local`
  hosts, so `kind === "local"` in the API means "the hub's own machine".
- `GET /api/v1/hosts` already returns `kind`, `connected` and `last_seen`
  (`crates/domovoi-hub/src/api/hosts.rs`, `host_dto`).
- Without a local agent nothing ever connects for it, so `HostStatus`
  (`web/src/features/hosts/HostStatus.tsx`) renders "Never connected" and `HostPills`
  (`web/src/app/Layout.tsx`) renders a red dot.

No backend change is needed.

## Read these files

- `web/src/features/hosts/HostStatus.tsx`
- `web/src/features/hosts/HostDetailPage.tsx`
- `web/src/features/hosts/HostsPage.tsx`
- `web/src/app/Layout.tsx` and `web/src/app/Layout.module.css`
- `web/src/features/hosts/hosts.test.tsx`

## Steps

1. In `HostStatus.tsx` add and export
   `export function isHubOnly(host: Host): boolean { return host.kind === "local" && !host.connected && !host.last_seen; }`.
2. In `HostStatus`, before the "Never connected" branch, return for `isHubOnly(host)`:
   `<span className={styles.status} data-connected="hub" title="The hub records its own problems here. No agent watches this machine.">Hub only, no agent</span>`.
   In `hosts.module.css`, add `.status[data-connected="hub"]::before { background: var(--muted); }`
   next to the existing `.status[data-connected="true"]::before` rule (the default dot is
   the danger colour).
3. In `Layout.tsx` `HostPills`, set `data-connected={isHubOnly(host) ? "hub" : host.connected}`
   and add `.pill[data-connected="hub"] .dot { background: var(--muted); }` to
   `Layout.module.css`. The hidden text becomes " hub only" for that case.
4. In `HostDetailPage.tsx`, when `isHubOnly(detail.host)`, show a short note under the
   heading:
   > This is the hub's own machine. Domovoi records hub problems here, such as the hub
   > losing its network. To also watch this machine's disks, services and containers,
   > install an agent on it and enroll it with the name **{name}**, or run the hub with
   > `--with-local-agent /etc/domovoi/agent.toml` (see `deploy/systemd/single-box.conf`).
5. Add one sentence with the same meaning to `docs/book/src/troubleshooting.md` under a
   heading "The hub's own host says Hub only".

## Tests

In `web/src/features/hosts/hosts.test.tsx` add
`hub_host_without_agent_is_shown_as_hub_only`: a host with `kind: "local"`,
`connected: false`, `last_seen: null` renders "Hub only, no agent" and not
"Never connected"; a `kind: "remote"` host with the same values still renders
"Never connected".

## Done when

- The hub host shows "Hub only, no agent" with a grey dot, and its page explains how to
  add an agent.
- A local host whose agent connected before and went away still shows "Offline, last seen".
- `npm run lint && npm run typecheck && npm test` pass.

## Do not

- Do not delete or hide the hub host: incidents are attached to it.
- Do not change the API or the Rust code.
