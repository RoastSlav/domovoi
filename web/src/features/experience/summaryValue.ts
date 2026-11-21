import { operationStatusMeta } from "../../design/tone";

const CUT = 400;

function compact(value: unknown): string {
  let text: string;
  try {
    text = JSON.stringify(value) ?? String(value);
  } catch {
    text = String(value);
  }
  return text.length > CUT ? `${text.slice(0, CUT)}…` : text;
}

function record(value: unknown): Record<string, unknown> | undefined {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;
}

function hostName(id: unknown, hostNames: ReadonlyMap<string, string>): string | undefined {
  return typeof id === "string" ? (hostNames.get(id) ?? "an unknown host") : undefined;
}

function each(value: unknown, line: (item: unknown) => string | undefined): string | undefined {
  if (!Array.isArray(value)) {
    return undefined;
  }
  const lines = value.map(line);
  return lines.every((l) => l !== undefined) ? lines.join("; ") : undefined;
}

function subject(item: unknown, hostNames: ReadonlyMap<string, string>): string | undefined {
  const shape = record(item);
  if (typeof shape?.check !== "string") {
    return undefined;
  }
  const where = typeof shape.target === "string" ? shape.target : hostName(shape.host, hostNames);
  return where === undefined ? shape.check : `${shape.check} on ${where}`;
}

function contract(item: unknown, hostNames: ReadonlyMap<string, string>): string | undefined {
  const shape = record(item);
  if (typeof shape?.contract !== "string" || typeof shape.outcome !== "string") {
    return undefined;
  }
  const where = typeof shape.target === "string" ? shape.target : hostName(shape.host, hostNames);
  return `${shape.contract}${where === undefined ? "" : ` on ${where}`}: ${shape.outcome}`;
}

/**
 * The text of one resolution-summary field. `hostNames` maps host ids to names. A value of a
 * shape this page does not expect shows as compact JSON cut at 400 characters; this never throws.
 */
export function summaryValue(
  field: string,
  value: unknown,
  hostNames: ReadonlyMap<string, string>,
): string {
  let text: string | undefined;
  switch (field) {
    case "outcome":
      text = typeof value === "string" ? operationStatusMeta(value).label : undefined;
      break;
    case "verified":
      text = typeof value === "boolean" ? (value ? "Yes" : "No") : undefined;
      break;
    case "subjects":
      text = each(value, (item) => subject(item, hostNames));
      break;
    case "contracts_executed":
      text = each(value, (item) => contract(item, hostNames));
      break;
    case "hosts":
      text = each(value, (id) => hostName(id, hostNames));
      break;
    default:
      text = typeof value === "string" ? value : undefined;
  }
  return text ?? compact(value);
}
