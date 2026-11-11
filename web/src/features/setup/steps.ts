import type { TestTarget } from "../../api/hooks/settings";
import type { FieldSpec } from "../settings/form";

/** One step of the first run: a group of settings, saved together with the others. */
export interface Step {
  id: "hub" | "discord" | "model" | "digest";
  title: string;
  intro: string;
  fields: FieldSpec[];
  test?: TestTarget;
  optional: boolean;
  /** What skipping this step leaves out, shown on the Skip button. */
  skipHint: string;
  /** The Settings section that holds these values later (`/settings/<section>`). */
  section: string;
}

/** The steps before the first host; their settings are applied together. */
export const STEPS: Step[] = [
  {
    id: "hub",
    title: "Hub",
    intro: "What this hub is called and where people reach it.",
    optional: false,
    skipHint: "",
    section: "hub",
    fields: [
      { path: "hub.name", label: "Name", kind: "text" },
      { path: "hub.timezone", label: "Time zone", kind: "text" },
      { path: "web.public_url", label: "Public URL", kind: "text" },
    ],
  },
  {
    id: "discord",
    title: "Discord",
    intro: "Incidents arrive as cards in a channel, with buttons to acknowledge and approve.",
    optional: true,
    test: "discord",
    skipHint: "Incidents will not be posted to Discord. You can add it later in Settings.",
    section: "discord",
    fields: [
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
    ],
  },
  {
    id: "model",
    title: "Model",
    intro: "A model triages each incident: what happened, why, and what to do.",
    optional: true,
    test: "llm",
    skipHint:
      "Incidents will not be triaged until you add a model. You can add one later in Settings.",
    section: "models",
    fields: [
      {
        path: "llm.primary.kind",
        label: "Provider",
        kind: "select",
        options: ["", "anthropic", "openai_compatible", "cli"],
      },
      {
        path: "llm.primary.model",
        label: "Model",
        kind: "model",
        visibleWhen: { path: "llm.primary.kind", values: ["anthropic", "openai_compatible"] },
      },
      {
        path: "llm.primary.base_url",
        label: "Base URL",
        kind: "text",
        placeholder: "http://localhost:11434/v1",
        hint: "API root. Required for OpenAI-compatible providers; optional for Anthropic proxies.",
        visibleWhen: { path: "llm.primary.kind", values: ["anthropic", "openai_compatible"] },
      },
      {
        path: "llm.primary.api_key",
        label: "API key",
        kind: "secret",
        visibleWhen: { path: "llm.primary.kind", values: ["anthropic", "openai_compatible"] },
      },
      {
        path: "llm.primary.cli.preset",
        label: "CLI preset",
        kind: "select",
        options: ["", "claude", "codex"],
        visibleWhen: { path: "llm.primary.kind", values: ["cli"] },
      },
      { path: "triage.monthly_budget_usd", label: "Monthly budget (USD)", kind: "number" },
      { path: "triage.enabled", label: "Triage incidents with this model", kind: "bool" },
    ],
  },
  {
    id: "digest",
    title: "Digest and quiet hours",
    intro: "A daily summary, and hours when only critical alerts get through.",
    optional: true,
    skipHint:
      "No daily digest is sent and quiet hours stay off. You can set them later in Settings.",
    section: "notifications",
    fields: [
      { path: "notify.digest.time", label: "Digest time", kind: "text", placeholder: "09:00" },
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
    ],
  },
];

/** The name of the last stage, after the settings steps. */
export const HOST_TITLE = "First host";

/** The browser's time zone, the best first guess for the hub's. */
export function browserTimeZone(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone;
  } catch {
    return "UTC";
  }
}
