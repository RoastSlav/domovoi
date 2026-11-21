import { subjectLabel, type SubjectNames } from "./subjects";

const CUT = 400;
const GIB = 1024 ** 3;

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

function utc(value: unknown): string | undefined {
  if (typeof value !== "string") {
    return undefined;
  }
  const at = new Date(value);
  return Number.isNaN(at.getTime()) ? undefined : at.toISOString().slice(0, 16).replace("T", " ");
}

function maintenance(shape: Record<string, unknown>): string | undefined {
  const start = utc(shape.starts_at);
  const end = utc(shape.ends_at);
  if (start === undefined || end === undefined) {
    return undefined;
  }
  const note = typeof shape.note === "string" && shape.note !== "" ? ` (${shape.note})` : "";
  return `${start} to ${end} UTC${note}`;
}

function platform(shape: Record<string, unknown>): string | undefined {
  const { hostname, os, kernel, arch, cpu_count: cpus, mem_total_bytes: mem } = shape;
  if (
    typeof hostname !== "string" ||
    typeof os !== "string" ||
    typeof kernel !== "string" ||
    typeof arch !== "string" ||
    typeof cpus !== "number" ||
    typeof mem !== "number"
  ) {
    return undefined;
  }
  const cpu = `${cpus} ${cpus === 1 ? "CPU" : "CPUs"}`;
  return [hostname, os, kernel, arch, cpu, `${(mem / GIB).toFixed(1)} GiB`].join(" · ");
}

/**
 * A fact's value as one line of text. A value of a shape the page does not know shows as
 * compact JSON cut at 400 characters; this never throws.
 */
export function valueText(predicate: string, value: unknown, names: SubjectNames): string {
  if (value === null || value === undefined) {
    return "";
  }
  const shape = record(value);
  let text: string | undefined;
  switch (predicate) {
    case "management_path":
    case "trusted_cidr":
      text = typeof shape?.cidr === "string" ? shape.cidr : undefined;
      break;
    case "maintenance_window":
      text = shape === undefined ? undefined : maintenance(shape);
      break;
    case "host_platform":
      text = shape === undefined ? undefined : platform(shape);
      break;
    case "never_restart":
      text = value === true ? "Never restart" : undefined;
      break;
    case "depends_on":
      text = typeof value === "string" ? subjectLabel(value, names).text : undefined;
      break;
    default:
      text = typeof value === "string" ? value : undefined;
  }
  return text ?? compact(value);
}
