import { fixed, type SectionSpec } from "./types";

/** Discord: the bot that posts incidents and takes approvals. */
export const DISCORD_SECTION: SectionSpec = {
  id: "discord",
  title: "Discord",
  intro: "The bot that posts incidents and takes approvals, and the webhook for digests.",
  test: "discord",
  extra: "discord-help",
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
};

/** What the hub receives from and pings outside itself: webhooks, probes, the heartbeat. */
export const OUTSIDE_SECTIONS: SectionSpec[] = [
  {
    id: "webhooks",
    title: "Webhooks",
    intro: "Endpoints that let other tools report problems to the hub.",
    extra: "add-webhook",
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
    id: "probes",
    title: "Probes",
    intro: "Checks the hub runs itself against addresses it can reach.",
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
    id: "heartbeat",
    title: "Heartbeat",
    intro: "A URL the hub pings on a schedule, so an outside service can tell when it stops.",
    test: "heartbeat",
    fields: fixed([
      { path: "heartbeat.url", label: "Heartbeat URL", kind: "secret" },
      { path: "heartbeat.interval", label: "Interval", kind: "text", placeholder: "1m" },
    ]),
  },
];
