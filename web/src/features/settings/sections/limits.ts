import { fixed, type SectionSpec } from "./types";

/** Sections for the limits of unattended actions, operations and the metrics endpoint. */
export const LIMIT_SECTIONS: SectionSpec[] = [
  {
    id: "policy",
    title: "Policy limits",
    intro:
      "How long a permit lasts, how fast the hub may use them and when a failing rule is suspended. Changing a limit asks for your password. The rules themselves are on the Policy page.",
    fields: fixed([
      {
        path: "policy.shadow",
        label: "Record policy decisions",
        kind: "bool",
        hint: "Write down what the policy would decide without acting on it.",
      },
      { path: "policy.permit_ttl", label: "Permit lifetime", kind: "text", placeholder: "5m" },
      {
        path: "policy.circuit_failures",
        label: "Failures before a rule is suspended",
        kind: "number",
      },
      { path: "policy.circuit_window", label: "Failure window", kind: "text", placeholder: "1h" },
      { path: "policy.max_permits_per_hour", label: "Permits per hour", kind: "number" },
    ]),
  },
  {
    id: "operations",
    title: "Operations",
    intro:
      "Durable operations that wait for a person, run skills and verify the result, and what one operation may spend before it pauses.",
    fields: fixed([
      {
        path: "orchestration.enabled",
        label: "Investigate incidents as operations",
        kind: "bool",
        hint: "Off stops automatic investigation; alert cards still go out.",
      },
      { path: "orchestration.max_running", label: "Operations at once", kind: "number" },
      { path: "orchestration.approval_ttl", label: "Approval waits for", kind: "text" },
      { path: "orchestration.question_timeout", label: "A question waits for", kind: "text" },
      { path: "orchestration.agent_wait_limit", label: "Wait for an agent", kind: "text" },
      {
        path: "orchestration.loop_guard_per_hour",
        label: "Runs of one skill per host per hour",
        kind: "number",
      },
      { path: "orchestration.budgets.model_turns", label: "Budget: model turns", kind: "number" },
      {
        path: "orchestration.budgets.evidence_requests",
        label: "Budget: evidence requests",
        kind: "number",
      },
      { path: "orchestration.budgets.executions", label: "Budget: executions", kind: "number" },
      { path: "orchestration.budgets.cycles", label: "Budget: repair cycles", kind: "number" },
      { path: "orchestration.budgets.wall_time", label: "Budget: wall-clock time", kind: "text" },
      { path: "orchestration.budgets.tokens", label: "Budget: model tokens", kind: "number" },
    ]),
  },
  {
    id: "metrics",
    title: "Metrics",
    intro:
      "A Prometheus scraper sends this token as a bearer token. Without one, only a signed-in web session can read /metrics. Changing it asks for your password.",
    fields: fixed([{ path: "metrics.token", label: "Scrape token", kind: "secret" }]),
  },
];
