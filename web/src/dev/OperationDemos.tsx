import type { Action, Execution } from "../api/types";
import { Grid, SidePanel, Text } from "../components";
import { ApprovalCard } from "../features/approvals/ApprovalCard";
import { Demo, Group } from "./Demo";
import styles from "./design.module.css";

const NOW = Date.now();
const HASH = "c4e1a7d2".repeat(8);

function step(overrides: Partial<Execution>): Execution {
  return {
    step: `demo-${overrides.state}-${overrides.risk}`,
    approval_step: "demo-approval",
    contract: "skill.restart_container",
    contract_version: 1,
    host_id: "demo",
    check: "container.down",
    target: "plex",
    args: { name: "plex" },
    state: "awaiting_approval",
    plan_hash: HASH,
    plan_binding: "v2",
    risk: "write",
    confirm_required: false,
    expires_at: new Date(NOW + 45 * 60_000).toISOString(),
    approved_by: null,
    approved_at: null,
    reason: null,
    ...overrides,
  };
}

const LEGACY_V1: Action = {
  id: "demo-legacy",
  incident_id: "demo",
  host_id: "demo",
  skill: "restart_container",
  risk: "write",
  args: { name: "plex" },
  status: "proposed",
  plan_hash: HASH,
  plan_hash_v1: HASH,
  plan_binding: "v1",
  proposed_by: "system:triage",
  expires_at: new Date(NOW + 45 * 60_000).toISOString(),
  created_at: new Date(NOW - 15 * 60_000).toISOString(),
  updated_at: new Date(NOW).toISOString(),
  legacy: true,
} as Action;

function Card({
  execution,
  ...rest
}: { execution: Execution } & Partial<Parameters<typeof ApprovalCard>[0]>) {
  return (
    <ApprovalCard
      source={{ kind: "step", operationId: "demo-operation", execution }}
      host="nas-01"
      {...rest}
    />
  );
}

/** The side panel chrome and the operation approval card, in every state a step can show. */
export function OperationDemos() {
  return (
    <Group title="Operations">
      <Demo name="SidePanel: side (a column beside the page) and sheet (full screen on a phone)">
        <Grid min="lg">
          <div className={styles.panelFrame}>
            <SidePanel title="Step inspector" onClose={() => undefined}>
              <Text block>Its body scrolls on its own.</Text>
            </SidePanel>
          </div>
          <div className={styles.panelFrame}>
            <SidePanel title="Step inspector" mode="sheet" onClose={() => undefined}>
              <Text block>The sheet fills its frame here; on a phone it fills the screen.</Text>
            </SidePanel>
          </div>
        </Grid>
      </Demo>
      <Demo name="ApprovalCard: waiting, destructive, unknown risk, read-only (approve and deny call the hub, so they fail here)">
        <Grid min="lg">
          <Card execution={step({})} why="The container stopped answering health checks." />
          <Card
            execution={step({
              risk: "destructive",
              confirm_required: true,
              contract: "skill.prune",
            })}
          />
          <Card execution={step({ risk: null, confirm_required: true })} />
          <Card execution={step({})} readOnly />
          <Card
            execution={step({})}
            extras={{
              effect: { class: "restarts a service", reversible: true },
              policy: { outcome: "needs approval", rule: "restarts" },
            }}
          />
        </Grid>
      </Demo>
      <Demo name="ApprovalCard: decided, in flight, refused, never ran, unavailable, result unknown">
        <Grid min="lg">
          <Card
            execution={step({
              state: "succeeded",
              approved_by: "user:1",
              approved_at: new Date(NOW - 60_000).toISOString(),
            })}
          />
          <Card execution={step({ state: "authorized" })} />
          <Card execution={step({ state: "dispatched" })} />
          <Card
            execution={step({ state: "refused", reason: "The agent refused: the plan changed." })}
            extras={{ refusal: { reason: "The skill is no longer allowed on this host." } }}
          />
          <Card execution={step({ state: "not_run", reason: "Nobody approved it in time." })} />
          <Card execution={step({ state: "unavailable", reason: "The host was offline." })} />
          <Card execution={step({ state: "outcome_unknown", reason: "The agent went away." })} />
        </Grid>
      </Demo>
      <Demo name="ApprovalCard: a classic action keeps its v1 binding label">
        <Grid min="lg">
          <ApprovalCard source={{ kind: "legacy", action: LEGACY_V1 }} host="nas-01" />
        </Grid>
      </Demo>
    </Group>
  );
}
