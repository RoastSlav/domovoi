import type { Action, Host, IncidentDetail, IncidentSummary } from "../api/types";

/** Times the fixtures use. */
export const T0 = "2026-10-31T12:00:00Z";

/** The `nas` host, connected. */
export const nas: Host = { id: "host-nas", name: "nas", kind: "agent", connected: true };

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

/** A proposed action, overridable. */
export function action(overrides: Partial<Action> = {}): Action {
  return {
    id: "act-1",
    incident_id: "inc-1",
    host_id: nas.id,
    skill: "restart_container",
    risk: "write",
    args: { name: "web" },
    status: "proposed",
    proposed_by: "system:triage",
    expires_at: "2026-10-31T13:00:00Z",
    created_at: T0,
    updated_at: T0,
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
    actions: [action()],
  };
}
