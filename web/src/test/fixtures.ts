import type {
  AuditRow,
  AuditVerification,
  Host,
  IncidentDetail,
  IncidentSummary,
  Overview,
} from "../api/types";

/** Times the fixtures use. */
export const T0 = "2026-10-31T12:00:00Z";

/** The `nas` host, connected. */
export const nas: Host = {
  id: "host-nas",
  name: "nas",
  kind: "remote",
  connected: true,
  agent_enrolled: true,
  version: "0.0.1",
  outdated: false,
  protocol_state: "compatible",
  reported_protocol: "1.1",
  hub_protocol: "1.1",
  reported_release: "0.0.1",
  incompatible_since: null,
  open_incidents: 1,
};

/** An open incident on `nas`, overridable. */
export function incident(overrides: Partial<IncidentSummary> = {}): IncidentSummary {
  return {
    id: "inc-1",
    host_id: nas.id,
    check: "disk.usage",
    target: "/srv",
    title: "Disk /srv is 93% full",
    status: "open",
    severity: "warning",
    verdict: null,
    opened_at: T0,
    last_firing_at: T0,
    occurrences: 1,
    ...overrides,
  };
}

/** The detail of `incident()` after triage. */
export function detail(): IncidentDetail {
  return {
    incident: incident({ verdict: "ACTION_NEEDED" }),
    host: "nas",
    report: null,
    triage: { kind: "report" },
    operations: [],
    card: {
      badge: "ACTION_NEEDED",
      headline: "Backups filled /srv",
      bullets: ["93% used, 70 GB free", "Grew 40 GB since 02:00"],
      next_step: "Prune old snapshots",
      details: "The nightly job kept **every** snapshot.",
      resolved: false,
    },
    timeline: [
      { at: T0, kind: "incident.opened", summary: "Disk /srv crossed 90%" },
      { at: "2026-10-31T12:01:00Z", kind: "triage.report", summary: "ACTION_NEEDED" },
    ],
    tools_used: [],
    as_of_seq: 0,
  };
}

/** The navigation's counts with nothing going on, overridable. */
export function overview(overrides: Partial<Overview> = {}): Overview {
  return {
    incidents_action_needed: 0,
    incidents_open: 0,
    incident_groups: { needs_action: 0, monitor: 0, pending: 0, expected: 0, handled: 0 },
    hosts_offline: 0,
    hosts_total: 2,
    operations_running: 0,
    operations_waiting_approval: 0,
    operations_waiting_user: 0,
    operations_paused: 0,
    attention_total: 0,
    as_of_seq: 0,
    ...overrides,
  };
}

/** Makes every media query match or not, as on a wide or a narrow screen. Undo with `vi.unstubAllGlobals()`. */
export function stubMedia(matches: boolean): void {
  vi.stubGlobal("matchMedia", (media: string) => ({
    matches,
    media,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
  }));
}

/** One audit entry; `overrides` change any field. */
export function auditRow(overrides: Partial<AuditRow> = {}): AuditRow {
  return {
    seq: 12,
    at: "2026-11-18T10:00:00Z",
    actor: "user:0190",
    kind: "action.approved",
    subject: "action:0190",
    detail: '{"plan_hash":"ab"}',
    detail_truncated: false,
    hash: "9f".repeat(32),
    ...overrides,
  };
}

/** A check of the audit log: chained intact. */
export function verification(overrides: Partial<AuditVerification> = {}): AuditVerification {
  return {
    scope: "recent",
    recent_limit: 10000,
    checked_at: "2026-11-18T10:00:00Z",
    chained: {
      status: "intact",
      checked: 812,
      head_seq: 817,
      head_hash: "9f".repeat(32),
      first_broken_seq: null,
      break_reason: null,
      break_text: null,
    },
    anchor: {
      install_id: "install-1",
      anchor_hash: "cd".repeat(32),
    },
    head: { seq: 817, hash: "9f".repeat(32) },
    ...overrides,
  };
}
