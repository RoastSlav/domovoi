import { fixed, type SectionSpec } from "./types";

/** The sections about how the hub runs and when it speaks up. */
export const HUB_SECTIONS: SectionSpec[] = [
  {
    id: "hub",
    title: "Hub",
    intro: "The hub's name, its addresses and how agents report in.",
    fields: fixed([
      { path: "hub.name", label: "Name", kind: "text" },
      { path: "hub.timezone", label: "Time zone", kind: "text", placeholder: "Europe/Sofia" },
      {
        path: "web.public_url",
        label: "Public URL",
        kind: "text",
        placeholder: "https://domovoi.example.net",
      },
      {
        path: "web.trusted_proxies",
        label: "Trusted proxies",
        kind: "list",
        placeholder: "127.0.0.1",
      },
      {
        path: "agent_server.public_url",
        label: "Agent server URL",
        kind: "text",
        placeholder: "wss://hub.lan:7421",
      },
      {
        path: "agent_server.heartbeat",
        label: "Agent heartbeat",
        kind: "text",
        placeholder: "10s",
        hint: "How often agents report in. Three missed heartbeats make an agent silent.",
      },
      {
        path: "agent_server.self_check",
        label: "Outside address to check",
        kind: "text",
        placeholder: "1.1.1.1:53",
        hint: "The hub tries this host:port before it blames a silent host, to tell a host outage from its own network.",
      },
    ]),
  },
  {
    id: "notifications",
    title: "Notifications",
    intro: "When the hub stays quiet, how often it reminds you and the daily digest.",
    fields: fixed([
      {
        path: "notify.quiet_hours.start",
        label: "Quiet hours start",
        kind: "text",
        placeholder: "00:00",
      },
      {
        path: "notify.quiet_hours.end",
        label: "Quiet hours end",
        kind: "text",
        placeholder: "09:00",
      },
      {
        path: "notify.renotify.warning",
        label: "Remind about warnings every",
        kind: "text",
        placeholder: "24h",
      },
      {
        path: "notify.renotify.critical",
        label: "Remind about critical every",
        kind: "text",
        placeholder: "4h",
      },
      { path: "notify.digest.enabled", label: "Send the daily digest", kind: "bool" },
      { path: "notify.digest.time", label: "Digest time", kind: "text", placeholder: "09:00" },
    ]),
  },
  {
    id: "incidents",
    title: "Incidents",
    intro: "When a failing check becomes an incident, and when an incident closes by itself.",
    fields: fixed([
      {
        path: "incidents.confirm_runs",
        label: "Failing runs before an incident opens",
        kind: "number",
      },
      { path: "incidents.reopen_window", label: "Reopen window", kind: "text", placeholder: "6h" },
      {
        path: "incidents.grouping_window",
        label: "Grouping window",
        kind: "text",
        placeholder: "5m",
      },
      {
        path: "incidents.default_ttl",
        label: "Resolve when quiet for",
        kind: "text",
        placeholder: "3d",
      },
    ]),
  },
];
