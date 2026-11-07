import type { TestTarget } from "../../api/hooks/settings";
import type { FieldSpec } from "./form";

/** One section of the settings page. */
export interface SectionSpec {
  title: string;
  fields: (values: Record<string, unknown>) => FieldSpec[];
  test?: TestTarget;
}

const fixed = (fields: FieldSpec[]) => () => fields;

/** The fields of one `[llm.*]` provider. `prefix` is `llm.primary` or `llm.secondary`. */
function providerFields(prefix: string): FieldSpec[] {
  const http = { path: `${prefix}.kind`, values: ["anthropic", "openai_compatible"] };
  const cli = { path: `${prefix}.kind`, values: ["cli"] };
  const primary = prefix === "llm.primary";
  return [
    {
      path: `${prefix}.kind`,
      label: "Provider",
      kind: "select",
      options: ["", "anthropic", "openai_compatible", "cli"],
      ...(primary
        ? {}
        : { hint: "Used when the main model is unavailable. Leave empty for none." }),
    },
    {
      path: `${prefix}.model`,
      label: "Model",
      kind: primary ? "model" : "text",
      visibleWhen: http,
    },
    {
      path: `${prefix}.base_url`,
      label: "Base URL",
      kind: "text",
      placeholder: "http://localhost:11434/v1",
      hint: "API root. Required for OpenAI-compatible providers; optional for Anthropic proxies.",
      visibleWhen: http,
    },
    { path: `${prefix}.api_key`, label: "API key", kind: "secret", visibleWhen: http },
    {
      path: `${prefix}.cli.preset`,
      label: "CLI preset",
      kind: "select",
      options: ["", "claude", "codex"],
      visibleWhen: cli,
    },
    {
      path: `${prefix}.cli.command`,
      label: "CLI program",
      kind: "text",
      placeholder: "/usr/local/bin/claude",
      visibleWhen: cli,
    },
    {
      path: `${prefix}.cli.args`,
      label: "CLI arguments",
      kind: "list",
      hint: "Replace the preset's own arguments when not empty. Separate with commas.",
      visibleWhen: cli,
    },
    {
      path: `${prefix}.timeout`,
      label: "Longest silence",
      kind: "text",
      placeholder: "2m",
      hint: "How long the model may produce nothing before the call fails. Answers are streamed, so the clock restarts with every token. For a CLI it is the whole run.",
      visibleWhen: { path: `${prefix}.kind`, values: ["anthropic", "openai_compatible", "cli"] },
    },
    {
      path: `${prefix}.price.input_per_mtok_usd`,
      label: "Input price (USD per million tokens)",
      kind: "number",
      visibleWhen: http,
    },
    {
      path: `${prefix}.price.output_per_mtok_usd`,
      label: "Output price (USD per million tokens)",
      kind: "number",
      visibleWhen: http,
    },
  ];
}

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
      {
        path: "discord.channel_id",
        label: "Channel ID",
        kind: "id",
        hint: "Numeric channel ID. With Developer Mode enabled, right-click the channel and choose Copy Channel ID.",
      },
      {
        path: "discord.allowed_user_ids",
        label: "Users who may approve",
        kind: "ids",
        hint: "Numeric Discord user IDs, separated by commas. Use Copy User ID; usernames, display names and tags do not work.",
      },
      {
        path: "discord.webhook_url",
        label: "Digest webhook URL",
        kind: "secret",
        hint: "Digests and quiet-hours summaries go here instead of through the bot. Leave empty to use the bot.",
      },
    ]),
  },
  {
    title: "Models and triage",
    test: "llm",
    fields: fixed([
      ...providerFields("llm.primary"),
      { path: "triage.enabled", label: "Triage incidents with the model", kind: "bool" },
      { path: "triage.monthly_budget_usd", label: "Monthly budget (USD)", kind: "number" },
      { path: "triage.concurrency", label: "Runs at once", kind: "number" },
      { path: "triage.per_hour_runs", label: "Runs per hour", kind: "number" },
      { path: "triage.cache_days", label: "Reuse a report for (days)", kind: "number" },
      { path: "triage.per_run.tool_calls", label: "Tool calls per run", kind: "number" },
      {
        path: "triage.per_run.seconds",
        label: "Seconds per run",
        kind: "number",
        hint: "After this long the model is told to answer with what it has. It does not cut a model call short.",
      },
      { path: "triage.per_run.output_tokens", label: "Output tokens per run", kind: "number" },
    ]),
  },
  {
    title: "Fallback model",
    fields: fixed(providerFields("llm.secondary")),
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
          path: `webhooks.${name}.host_label`,
          label: `${name}: host label`,
          kind: "text" as const,
          hint: "For Alertmanager, the label that names the host.",
        },
        {
          path: `webhooks.${name}.default_host`,
          label: `${name}: default host`,
          kind: "text" as const,
        },
      ]);
    },
  },
  {
    title: "Exposed secrets",
    fields: fixed([
      {
        path: "secrets.exposed",
        label: "Secrets known to be exposed",
        kind: "list",
        hint: "Names of secrets you know leaked. Each raises an advisory until you remove it here.",
      },
    ]),
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
