import type { FieldSpec } from "../form";

/** The fields of one `[llm.*]` provider. `prefix` is `llm.primary` or `llm.secondary`. */
export function providerFields(prefix: string): FieldSpec[] {
  const http = { path: `${prefix}.kind`, values: ["anthropic", "openai_compatible"] };
  const cli = { path: `${prefix}.kind`, values: ["cli"] };
  const primary = prefix === "llm.primary";
  return [
    {
      path: `${prefix}.kind`,
      label: "Provider",
      kind: "select",
      options: ["", "anthropic", "openai_compatible", "cli"],
      ...(primary ? {} : { hint: "Leave empty for none." }),
    },
    {
      path: `${prefix}.model`,
      label: "Model",
      kind: "model",
      ...(primary ? {} : { slot: "secondary" }),
      visibleWhen: http,
    },
    {
      path: `${prefix}.fallback_models`,
      label: "Fallback models",
      kind: "list",
      hint: "Tried in order when the model above fails. Comma-separated.",
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
