import type { Chip } from "../../components";
import { PREDICATES, SOURCES } from "../../design/knowledgeText";

/** The status filter of the facts list. */
export type StatusChip = "live" | "current" | "stale" | "disputed" | "superseded" | "retracted";

/** The status chips, in order; `live` is the default. */
export const STATUS_CHIPS: readonly Chip<StatusChip>[] = [
  { value: "live", label: "Live" },
  { value: "current", label: "Current" },
  { value: "stale", label: "Stale" },
  { value: "disputed", label: "Disputed" },
  { value: "superseded", label: "Replaced" },
  { value: "retracted", label: "Retracted" },
];

/** The `status` request parameter of a chip; `live` sends none, so the hub's default applies. */
export function statusParam(chip: StatusChip): string | undefined {
  return chip === "live" ? undefined : chip;
}

/** The filters of the facts list as they live in the address. */
export interface FactView {
  predicate: string;
  q: string;
  source: string;
  status: StatusChip;
}

const DEFAULTS: FactView = { status: "live", source: "", predicate: "", q: "" };

/** Reads `?status=`, `?source=`, `?predicate=` and `?q=`; an unknown or empty value is the default. */
export function readFactView(params: URLSearchParams): FactView {
  const status = params.get("status");
  const source = params.get("source") ?? "";
  const predicate = params.get("predicate") ?? "";
  return {
    status: STATUS_CHIPS.find((chip) => chip.value === status)?.value ?? DEFAULTS.status,
    source: Object.hasOwn(SOURCES, source) ? source : "",
    predicate: Object.hasOwn(PREDICATES, predicate) ? predicate : "",
    q: params.get("q") ?? "",
  };
}

/** The address after `change`: defaults leave it, and every other parameter (tab, host) stays. */
export function writeFactView(
  previous: URLSearchParams,
  change: Partial<FactView>,
): URLSearchParams {
  const next = new URLSearchParams(previous);
  for (const key of Object.keys(DEFAULTS) as (keyof FactView)[]) {
    if (!(key in change)) {
      continue;
    }
    const value = change[key] ?? DEFAULTS[key];
    if (value === DEFAULTS[key]) {
      next.delete(key);
    } else {
      next.set(key, value);
    }
  }
  return next;
}
