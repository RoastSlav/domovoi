import type { OperationTrace } from "../../api/operationTrace";
import type { BudgetKind } from "../../design/operationText";

/** A limit and what is used, in the units the hub sends (seconds, micros). */
export interface BudgetFigure {
  limit: number | null;
  used: number;
}

/** The limit and use of one budget kind in `trace`. Cycles are used up to the current cycle. */
export function limitFor(kind: BudgetKind, trace: OperationTrace): BudgetFigure {
  const { limits, used } = trace.operation.budgets;
  switch (kind) {
    case "model_turns":
      return { limit: limits.model_turns ?? null, used: used.model_turns };
    case "evidence_requests":
      return { limit: limits.evidence_requests ?? null, used: used.evidence_requests };
    case "executions":
      return { limit: limits.executions ?? null, used: used.executions };
    case "cycles":
      return { limit: limits.cycles ?? null, used: trace.operation.cycle };
    case "wall_time":
      return { limit: limits.wall_seconds ?? null, used: used.wall_seconds };
    case "tokens":
      return { limit: limits.tokens ?? null, used: used.tokens };
    case "cost":
      return { limit: limits.cost_micros ?? null, used: used.cost_micros };
  }
}

/** Display units to API units: minutes to seconds, cost units to micros. */
export function toApiLimit(kind: BudgetKind, input: number): number {
  if (kind === "wall_time") {
    return Math.round(input * 60);
  }
  if (kind === "cost") {
    return Math.round(input * 1_000_000);
  }
  return Math.round(input);
}

/** API units to display units. */
export function fromApiLimit(kind: BudgetKind, api: number): number {
  if (kind === "wall_time") {
    return api / 60;
  }
  if (kind === "cost") {
    return api / 1_000_000;
  }
  return api;
}

function display(kind: BudgetKind, api: number): string {
  const value = fromApiLimit(kind, api);
  switch (kind) {
    case "wall_time":
      return `${Math.round(value * 10) / 10} min`;
    case "cost":
      return value.toFixed(2);
    case "tokens":
      return Math.round(value).toLocaleString("en-US");
    default:
      return String(value);
  }
}

/** "3 of 10", or "3 (no limit)". */
export function formatBudget(kind: BudgetKind, used: number, limit: number | null): string {
  return limit === null
    ? `${display(kind, used)} (no limit)`
    : `${display(kind, used)} of ${display(kind, limit)}`;
}

/**
 * Why `input` (display units) cannot be sent as a limit, or null when it can. Counts and tokens
 * are whole numbers and are never rounded for the person; time and cost are unit conversions
 * (minutes to whole seconds, cost to whole micros) and keep `toApiLimit`'s rounding.
 */
export function limitProblem(kind: BudgetKind, input: number): string | null {
  if (!Number.isFinite(input)) {
    return "Enter a number.";
  }
  if (kind !== "wall_time" && kind !== "cost" && !Number.isInteger(input)) {
    return "Enter a whole number.";
  }
  return null;
}
