import type { Action, OperationSummary } from "../../api/types";

/** One row of the Operations list. */
export interface OperationRow {
  kind: "operation" | "legacy";
  id: string;
  goal: string;
  status: string;
  hosts: string[];
  updatedAt: string;
  href: string;
}

/**
 * Operations and legacy actions in one list, newest first (ties: operations, then id). When a
 * source has more pages, rows older than its oldest loaded row are left out so none hides
 * behind an unloaded one.
 */
export function mergeRows(input: {
  operations: OperationSummary[];
  legacy: Action[];
  hostNames: Map<string, string>;
  moreOperations: boolean;
  moreLegacy: boolean;
}): OperationRow[] {
  const rows: OperationRow[] = [
    ...input.operations.map((operation): OperationRow => ({
      kind: "operation",
      id: operation.id,
      goal: operation.goal,
      status: operation.status,
      hosts: operation.hosts.map((host) => input.hostNames.get(host) ?? host),
      updatedAt: operation.updated_at,
      href: `/operations/${operation.id}`,
    })),
    ...input.legacy.map((action): OperationRow => {
      const host = input.hostNames.get(action.host_id);
      return {
        kind: "legacy",
        id: action.id,
        goal: `${action.skill} on ${host ?? "a host"}`,
        status: action.status,
        hosts: host ? [host] : [],
        updatedAt: action.updated_at,
        href: `/incidents/${action.incident_id}`,
      };
    }),
  ];
  const oldest = (kind: OperationRow["kind"]) =>
    rows
      .filter((row) => row.kind === kind)
      .reduce((min, row) => Math.min(min, Date.parse(row.updatedAt)), Infinity);
  const cut = Math.max(
    input.moreOperations ? oldest("operation") : -Infinity,
    input.moreLegacy ? oldest("legacy") : -Infinity,
  );
  return rows
    .filter((row) => Date.parse(row.updatedAt) >= cut)
    .sort(
      (a, b) =>
        Date.parse(b.updatedAt) - Date.parse(a.updatedAt) ||
        (a.kind === b.kind ? 0 : a.kind === "operation" ? -1 : 1) ||
        a.id.localeCompare(b.id),
    );
}
