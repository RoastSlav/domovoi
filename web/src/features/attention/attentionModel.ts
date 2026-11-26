import type { Action, Host, IncidentSummary, OperationSummary } from "../../api/types";
import { attentionWhy } from "../../design/attentionText";
import { containmentItems, type ContainmentAttentionInput } from "../containment/containmentItems";
import type { StatusMeta } from "../../design/status";
import {
  actionStatusMeta,
  hostConnectionMeta,
  hostConnectionOf,
  operationStatusMeta,
  severityMeta,
  stepOutcomeMeta,
} from "../../design/tone";

/** What an attention item is about. */
export type AttentionKind =
  "result_unknown" | "approval" | "question" | "paused" | "incident" | "host" | "containment";
/** Which list the item came from. */
export type AttentionSource = "operation" | "action" | "incident" | "host" | "containment";
/** The urgency tier: 0 act now, 1 waiting for you, 2 worth a look. */
export type Tier = 0 | 1 | 2;

/** The heading of each tier. */
export const TIER_TITLES: Readonly<Record<Tier, string>> = {
  0: "Act now",
  1: "Waiting for you",
  2: "Worth a look",
};

/** One thing that waits for the owner. */
export interface AttentionItem {
  key: string;
  kind: AttentionKind;
  source: AttentionSource;
  tier: Tier;
  /** The fixed ranking, 0 first. */
  rank: number;
  title: string;
  meta: StatusMeta;
  hostIds: string[];
  /** Where the decision is made. */
  href: string;
  /** RFC 3339, for ordering and the time shown. */
  since: string | null;
  /** Classic approvals only. */
  expiresAt: string | null;
  why: string;
}

/** What the items are built from: each source's loaded rows. */
export interface AttentionInputs {
  incidents: readonly IncidentSummary[];
  proposed: readonly Action[];
  unknownActions: readonly Action[];
  waitingApproval: readonly OperationSummary[];
  waitingUser: readonly OperationSummary[];
  paused: readonly OperationSummary[];
  hosts: readonly Host[];
  /** Each host's blocks: lost ones and a gate that refuses to block. */
  containment?: readonly ContainmentAttentionInput[];
  /** Host id to name. */
  hostNames: ReadonlyMap<string, string>;
}

/** Which attention state a host is in, if any; a host that never connected or the hub itself is none. */
export function hostAttention(
  host: Host,
): "incompatible" | "offline" | "expected_incompatible" | undefined {
  const connection = hostConnectionOf(host);
  return connection === "incompatible" ||
    connection === "offline" ||
    connection === "expected_incompatible"
    ? connection
    : undefined;
}

const SEVERITY_RANK: Record<string, number> = { critical: 3, warning: 9 };
const TIER_OF_RANK = [0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2] as const;

function item(rank: number, fields: Omit<AttentionItem, "rank" | "tier">): AttentionItem {
  return { ...fields, rank, tier: TIER_OF_RANK[rank] ?? 2 };
}

function operationItems(
  operations: readonly OperationSummary[],
  make: (op: OperationSummary) => AttentionItem,
): AttentionItem[] {
  return operations.map(make);
}

function operationBase(op: OperationSummary) {
  return {
    key: `operation:${op.id}`,
    source: "operation" as const,
    title: op.goal,
    hostIds: [...op.hosts],
    href: `/operations/${op.id}`,
    since: op.updated_at,
    expiresAt: null,
  };
}

function pausedItem(op: OperationSummary): AttentionItem {
  const reason = op.pause_reason?.type;
  if (reason === "outcome_unknown") {
    return item(0, {
      ...operationBase(op),
      kind: "result_unknown",
      meta: stepOutcomeMeta("outcome_unknown"),
      why: attentionWhy({ kind: "result_unknown_operation" }),
    });
  }
  return item(7, {
    ...operationBase(op),
    kind: "paused",
    meta: operationStatusMeta("paused"),
    why: attentionWhy({ kind: "paused", reason }),
  });
}

function hostItem(host: Host, state: NonNullable<ReturnType<typeof hostAttention>>): AttentionItem {
  const rank = state === "incompatible" ? 2 : state === "offline" ? 4 : 8;
  return item(rank, {
    key: `host:${host.id}`,
    kind: "host",
    source: "host",
    title: host.name,
    meta: hostConnectionMeta(state),
    hostIds: [],
    href: `/hosts/${host.name}`,
    since: host.incompatible_since ?? host.last_seen ?? null,
    expiresAt: null,
    why: attentionWhy({ kind: "host", connection: state }),
  });
}

function time(value: string | null): number | undefined {
  const t = value === null ? Number.NaN : Date.parse(value);
  return Number.isNaN(t) ? undefined : t;
}

/** Most recent first, unknown times last. */
function byRecency(a: AttentionItem, b: AttentionItem): number {
  const x = time(a.since);
  const y = time(b.since);
  if (x === y) {
    return 0;
  }
  return x === undefined ? 1 : y === undefined ? -1 : y - x;
}

/** Rank 5: operations first, longest waiting first; then classic actions by expiry. */
function byApprovalOrder(a: AttentionItem, b: AttentionItem): number {
  if (a.source !== b.source) {
    return a.source === "operation" ? -1 : 1;
  }
  if (a.source === "operation") {
    const x = time(a.since);
    const y = time(b.since);
    return x === y ? 0 : x === undefined ? 1 : y === undefined ? -1 : x - y;
  }
  const x = time(a.expiresAt);
  const y = time(b.expiresAt);
  return x === y ? 0 : x === undefined ? 1 : y === undefined ? -1 : x - y;
}

/** Every item the loaded sources hold, ordered by tier, then rank, then the order within the rank. */
export function attentionItems(inputs: AttentionInputs): AttentionItem[] {
  const items: AttentionItem[] = [
    ...operationItems(inputs.paused, pausedItem),
    ...containmentItems(inputs.containment ?? []),
    ...inputs.unknownActions.map((action) =>
      item(1, {
        key: `action:${action.id}`,
        kind: "result_unknown",
        source: "action",
        title: actionTitle(action, inputs.hostNames),
        meta: actionStatusMeta("outcome_unknown"),
        hostIds: [action.host_id],
        href: `/incidents/${action.incident_id}`,
        since: action.created_at,
        expiresAt: null,
        why: attentionWhy({ kind: "result_unknown_action" }),
      }),
    ),
    ...inputs.hosts.flatMap((host) => {
      const state = hostAttention(host);
      return state ? [hostItem(host, state)] : [];
    }),
    ...inputs.incidents.map((incident) =>
      item(SEVERITY_RANK[incident.severity] ?? 10, {
        key: `incident:${incident.id}`,
        kind: "incident",
        source: "incident",
        title: incident.title,
        meta: severityMeta(incident.severity),
        hostIds: [incident.host_id],
        href: `/incidents/${incident.id}`,
        since: incident.last_firing_at,
        expiresAt: null,
        why: "",
      }),
    ),
    ...operationItems(inputs.waitingApproval, (op) =>
      item(5, {
        ...operationBase(op),
        kind: "approval",
        meta: operationStatusMeta("waiting_approval"),
        why: "",
      }),
    ),
    ...inputs.proposed.map((action) =>
      item(5, {
        key: `action:${action.id}`,
        kind: "approval",
        source: "action",
        title: actionTitle(action, inputs.hostNames),
        meta: actionStatusMeta("proposed"),
        hostIds: [action.host_id],
        href: "/approvals",
        since: action.created_at,
        expiresAt: action.expires_at,
        why: "",
      }),
    ),
    ...operationItems(inputs.waitingUser, (op) =>
      item(6, {
        ...operationBase(op),
        kind: "question",
        meta: operationStatusMeta("waiting_user"),
        why: attentionWhy({ kind: "question" }),
      }),
    ),
  ];
  return items.sort(
    (a, b) =>
      a.tier - b.tier ||
      a.rank - b.rank ||
      (a.rank === 5 ? byApprovalOrder(a, b) : byRecency(a, b)) ||
      a.key.localeCompare(b.key),
  );
}

function actionTitle(action: Action, hostNames: ReadonlyMap<string, string>): string {
  return `${action.skill} on ${hostNames.get(action.host_id) ?? "a host"}`;
}
