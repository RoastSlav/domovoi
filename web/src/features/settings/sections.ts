import type { TestTarget } from "../../api/hooks/settings";
import type { FieldSpec } from "./form";

/** One section of the settings page. */
export interface SectionSpec {
  title: string;
  fields: (values: Record<string, unknown>) => FieldSpec[];
  test?: TestTarget;
}

const fixed = (fields: FieldSpec[]) => () => fields;

/** The hub's sections, in page order. */
export const SECTIONS: SectionSpec[] = [
  {
    title: "Hub",
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
    ]),
  },
  {
    title: "Notifications",
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
    title: "Incidents",
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
  {
    title: "Discord",
    test: "discord",
    fields: fixed([
      { path: "discord.enabled", label: "Run the bot", kind: "bool" },
      { path: "discord.bot_token", label: "Bot token", kind: "secret" },
      { path: "discord.channel_id", label: "Channel id", kind: "ids" },
      { path: "discord.allowed_user_ids", label: "Users who may approve", kind: "ids" },
      { path: "discord.webhook_url", label: "Fallback webhook URL", kind: "secret" },
    ]),
  },
  {
    title: "Models and triage",
    test: "llm",
    fields: fixed([
      {
        path: "llm.primary.kind",
        label: "Provider",
        kind: "select",
        options: ["", "anthropic", "openai_compatible", "cli"],
      },
      { path: "llm.primary.model", label: "Model", kind: "text" },
      { path: "llm.primary.base_url", label: "Endpoint", kind: "text" },
      { path: "llm.primary.api_key", label: "API key", kind: "secret" },
      {
        path: "llm.primary.cli.preset",
        label: "CLI preset",
        kind: "select",
        options: ["", "claude", "codex"],
      },
      {
        path: "llm.primary.cli.command",
        label: "CLI program",
        kind: "text",
        placeholder: "/usr/local/bin/claude",
      },
      { path: "triage.enabled", label: "Triage incidents with the model", kind: "bool" },
      { path: "triage.monthly_budget_usd", label: "Monthly budget (USD)", kind: "number" },
      { path: "triage.concurrency", label: "Runs at once", kind: "number" },
      { path: "triage.per_hour_runs", label: "Runs per hour", kind: "number" },
    ]),
  },
  {
    title: "Webhooks",
    fields: (values) => {
      const hooks = (values.webhooks ?? {}) as Record<string, unknown>;
      return Object.keys(hooks).flatMap((name) => [
        {
          path: `webhooks.${name}.kind`,
          label: `${name}: kind`,
          kind: "select" as const,
          options: ["", "generic", "alertmanager", "uptime_kuma"],
        },
        {
          path: `webhooks.${name}.auth`,
          label: `${name}: authentication`,
          kind: "select" as const,
          options: ["", "hmac", "bearer"],
        },
        { path: `webhooks.${name}.secret`, label: `${name}: secret`, kind: "secret" as const },
        {
          path: `webhooks.${name}.default_host`,
          label: `${name}: default host`,
          kind: "text" as const,
        },
      ]);
    },
  },
  {
    title: "Probes",
    fields: fixed([
      {
        path: "probes",
        label: "Probes",
        kind: "json",
        hint: 'A list such as [{ "host": "nas", "kind": "tcp", "target": "nas.lan:22" }].',
      },
    ]),
  },
  {
    title: "Heartbeat",
    test: "heartbeat",
    fields: fixed([
      { path: "heartbeat.url", label: "Heartbeat URL", kind: "secret" },
      { path: "heartbeat.interval", label: "Interval", kind: "text", placeholder: "1m" },
    ]),
  },
  {
    title: "Backups",
    fields: fixed([
      { path: "backup.dir", label: "Backup directory", kind: "text" },
      { path: "backup.keep", label: "Backups to keep", kind: "number" },
    ]),
  },
];
