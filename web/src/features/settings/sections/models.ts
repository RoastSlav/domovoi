import { providerFields } from "./providers";
import { fixed, type SectionSpec } from "./types";

/** The sections about the language model that triages incidents. */
export const MODEL_SECTIONS: SectionSpec[] = [
  {
    id: "models",
    title: "Models and triage",
    intro: "The main model, and how much work triage may do with it.",
    test: "llm",
    fields: fixed([
      ...providerFields("llm.primary"),
      { path: "triage.enabled", label: "Triage incidents with the model", kind: "bool" },
      { path: "triage.monthly_budget_usd", label: "Monthly budget (USD)", kind: "number" },
      { path: "triage.concurrency", label: "Runs at once", kind: "number" },
      { path: "triage.per_hour_runs", label: "Runs per hour", kind: "number" },
      {
        path: "triage.reserved_for_incidents",
        label: "Runs kept for triage",
        kind: "number",
      },
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
    id: "fallback-model",
    title: "Fallback model",
    intro:
      "Used when the main model fails, times out, is rate limited or gives an unusable answer. Optional.",
    test: "llm",
    testSlot: "secondary",
    fields: fixed(providerFields("llm.secondary")),
  },
];
