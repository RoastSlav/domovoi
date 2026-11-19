/** The kinds of budget an operation has. */
export type BudgetKind =
  "model_turns" | "evidence_requests" | "executions" | "cycles" | "wall_time" | "tokens" | "cost";

/** Every budget kind, in the order the details list them. */
export const BUDGET_KINDS: readonly BudgetKind[] = [
  "model_turns",
  "evidence_requests",
  "executions",
  "cycles",
  "wall_time",
  "tokens",
  "cost",
];

const BUDGET_LABELS: Record<BudgetKind, string> = {
  model_turns: "Model turns",
  evidence_requests: "Evidence requests",
  executions: "Actions",
  cycles: "Cycles",
  wall_time: "Time",
  tokens: "Tokens",
  cost: "Cost",
};

const WAIT_TEXT: Record<string, string> = {
  approval: "waiting for your approval",
  user: "waiting for your answer",
  agent: "waiting for the host's agent",
  timer: "waiting for a scheduled check",
};

/** What a budget kind is called; an unknown kind shows as given. */
export function budgetLabel(kind: string): string {
  return Object.hasOwn(BUDGET_LABELS, kind) ? BUDGET_LABELS[kind as BudgetKind] : kind;
}

/** What an operation waits for, in words; an unknown kind reads "waiting". */
export function waitText(kind: string): string {
  return Object.hasOwn(WAIT_TEXT, kind) ? (WAIT_TEXT[kind] ?? "waiting") : "waiting";
}

/** The label of an edge seen from the step it points at. */
export const REVERSE_EDGE_LABELS: Record<string, string> = {
  depends_on: "Needed by",
  caused_by: "Led to",
  verifies: "Verified by",
  retry_of: "Retried by",
  answers: "Answered by",
};

/** The incoming label of an edge kind; an unknown kind shows as given. */
export function reverseEdgeLabel(kind: string): string {
  return Object.hasOwn(REVERSE_EDGE_LABELS, kind) ? (REVERSE_EDGE_LABELS[kind] ?? kind) : kind;
}
