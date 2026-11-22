import type { Action, Host, IncidentSummary, OperationSummary } from "../api/types";
import { Stack } from "../components";
import { attentionItems, TIER_TITLES, type Tier } from "../features/attention/attentionModel";
import { AttentionRows } from "../features/attention/AttentionRows";
import { SourceFailures } from "../features/attention/SourceNotices";
import { Demo, Group } from "./Demo";

const NOW = Date.now();
const ago = (minutes: number) => new Date(NOW - minutes * 60_000).toISOString();

function operation(id: string, status: string, over: Partial<OperationSummary>) {
  return {
    id,
    kind: "investigate",
    goal: `Demo operation ${id}`,
    status,
    pause_reason: null,
    waiting_for: [],
    cycle: 0,
    incidents: [],
    hosts: ["demo-nas"],
    created_at: ago(90),
    updated_at: ago(10),
    ...over,
  } as OperationSummary;
}

function host(id: string, over: Partial<Host>): Host {
  return {
    id,
    name: id,
    kind: "remote",
    connected: false,
    agent_enrolled: true,
    outdated: false,
    open_incidents: 0,
    last_seen: ago(120),
    ...over,
  } as Host;
}

const INCIDENTS = [
  { id: "i1", title: "Disk /srv is 93% full", severity: "critical", last_firing_at: ago(30) },
  { id: "i2", title: "Backup ran late", severity: "warning", last_firing_at: ago(200) },
  { id: "i3", title: "A new kind of signal", severity: "mystery", last_firing_at: ago(400) },
].map((one) => ({ ...one, host_id: "demo-nas" })) as IncidentSummary[];

const UNKNOWN_ACTION = {
  id: "a1",
  incident_id: "i1",
  host_id: "demo-nas",
  skill: "restart_container",
  status: "outcome_unknown",
  created_at: ago(50),
} as Action;

const ITEMS = attentionItems({
  incidents: INCIDENTS,
  proposed: [],
  unknownActions: [UNKNOWN_ACTION],
  waitingApproval: [],
  waitingUser: [operation("ask", "waiting_user", {})],
  paused: [
    operation("unknown", "paused", { pause_reason: { type: "outcome_unknown" } }),
    operation("budget", "paused", { pause_reason: { type: "budget" } }),
    operation("error", "paused", { pause_reason: { type: "error" } }),
    operation("manual", "paused", { pause_reason: { type: "manual" } }),
  ],
  hosts: [
    host("old-agent", { protocol_state: "incompatible" }),
    host("garage", {}),
    host("fresh", { protocol_state: "expected_incompatible" }),
  ],
  hostNames: new Map([["demo-nas", "nas-01"]]),
});

const TIERS: readonly Tier[] = [0, 1, 2];

/** The rows of the attention list in every kind, grouped by tier, and its partial-failure message. */
export function AttentionDemos() {
  return (
    <Group title="Attention">
      <Demo name="AttentionRows: result unknown, incompatible and offline hosts, paused (four reasons), a question, incidents (known and unknown severity)">
        <Stack gap={4}>
          {TIERS.map((tier) => (
            <div key={tier}>
              <h4>{TIER_TITLES[tier]}</h4>
              <AttentionRows
                items={ITEMS.filter((item) => item.tier === tier)}
                hostNames={new Map([["demo-nas", "nas-01"]])}
              />
            </div>
          ))}
        </Stack>
      </Demo>
      <Demo name="SourceFailures: two sources did not load, what did load is still listed">
        <SourceFailures
          failed={[
            { name: "incidents", error: new Error("demo"), retry: () => undefined },
            { name: "paused operations", error: new Error("demo"), retry: () => undefined },
          ]}
        />
      </Demo>
    </Group>
  );
}
