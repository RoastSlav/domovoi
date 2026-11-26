import type {
  Action,
  Host,
  IncidentSummary,
  OperationDetail,
  OperationSummary,
} from "../api/types";
import type { ContainmentStatus } from "../api/types";
import { route } from "./apiMock";
import { EMPTY_PAGE } from "./attentionFixtures";
import { containmentStatus } from "./containmentFixtures";
import { nas } from "./fixtures";
import { operationDetail } from "./operationFixtures";

/** What each attention source answers; an omitted source answers an empty first page. */
export interface Served {
  /** Each host's blocks by host name; a host not listed has none and no gate answer. */
  containment?: Record<string, ContainmentStatus>;
  actions?: Partial<Record<string, Action[]>>;
  details?: Record<string, OperationDetail>;
  hosts?: Host[];
  incidents?: IncidentSummary[];
  /** Answers by `status`, as the operations list is asked. */
  operations?: Partial<Record<string, OperationSummary[]>>;
  /** A next cursor for these operation statuses: the source has more pages. */
  more?: string[];
}

/** A waiting-approval operation detail under another id and goal, for lists of many. */
export function waitingDetail(id: string, goal: string): OperationDetail {
  const base = operationDetail("waiting-approval");
  return { ...base, operation: { ...base.operation, id, goal } };
}

/** Registers every route the attention sources and the approval cards read. */
export function serveAttention(served: Served = {}): void {
  route("GET /api/v1/hosts", () => served.hosts ?? [nas]);
  route(
    "GET /api/v1/hosts/{name}/containment",
    (init) => served.containment?.[init.params?.path?.name ?? ""] ?? containmentStatus(),
  );
  route("GET /api/v1/incidents", () => ({ ...EMPTY_PAGE, items: served.incidents ?? [] }));
  route("GET /api/v1/actions", (init) => ({
    ...EMPTY_PAGE,
    items: served.actions?.[String(init.params?.query?.status)] ?? [],
  }));
  route("GET /api/v1/operations", (init) => {
    const status = String(init.params?.query?.status);
    return {
      ...EMPTY_PAGE,
      items: served.operations?.[status] ?? [],
      next_cursor: served.more?.includes(status) ? "next" : null,
    };
  });
  route("GET /api/v1/operations/{id}", (init) => {
    const id = init.params?.path?.id ?? "";
    return served.details?.[id] ?? operationDetail("waiting-approval");
  });
}
