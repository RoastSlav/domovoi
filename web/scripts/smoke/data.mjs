/**
 * The mock hub's payloads. The operation comes from the golden trace fixture the vitest tests
 * share, so the smoke test and the unit tests read one contract.
 */
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const fixtures = join(
    dirname(fileURLToPath(import.meta.url)),
    "../../src/test/fixtures/operation-trace",
);
const T0 = "2026-10-31T12:00:00Z";

/** The full 64-hex plan hash the page must send back. */
export const PLAN_HASH = "ab".repeat(32);

function load(name) {
    return JSON.parse(readFileSync(join(fixtures, `${name}.json`), "utf8"));
}

/** A fresh, mutable state for one run: the waiting-approval operation, one legacy action. */
export function makeState({ destructive = false } = {}) {
    const trace = load("waiting-approval");
    const pinned = trace.operation.pinned[0];
    return {
        seq: trace.as_of_seq,
        trace,
        hostId: pinned.host,
        hostCheck: pinned.check,
        hostTarget: pinned.target,
        destructive,
        executionState: "awaiting_approval",
        approved: false,
    };
}

/** The operation summary the list and the detail carry. */
export function summary(state) {
    const op = state.trace.operation;
    return {
        id: op.id,
        kind: op.kind,
        goal: op.goal,
        status: op.status,
        pause_reason: op.pause_reason ?? null,
        waiting_for: op.waiting_for,
        cycle: op.cycle,
        incidents: op.incidents,
        hosts: [state.hostId],
        created_at: op.created_at,
        started_at: op.started_at ?? null,
        ended_at: op.ended_at ?? null,
        updated_at: T0,
    };
}

/** The execution the card shows; the skill is destructive in the variant. */
export function executions(state) {
    const step = state.trace.steps.find((candidate) => candidate.kind === "execution");
    if (!step) {
        return [];
    }
    return [
        {
            step: step.id,
            approval_step: null,
            contract: state.destructive ? "skill.prune_volumes" : "skill.restart_container",
            contract_version: 1,
            host_id: state.hostId,
            check: state.hostCheck,
            target: state.hostTarget,
            args: { name: state.hostTarget },
            state: state.executionState,
            plan_hash: PLAN_HASH,
            plan_binding: "v2",
            risk: state.destructive ? "destructive" : "write",
            confirm_required: state.destructive,
            expires_at: "2099-01-01T00:00:00Z",
            approved_by: state.approved ? "user:1" : null,
            approved_at: state.approved ? T0 : null,
            reason: null,
        },
    ];
}

/** `GET /operations/{id}`. */
export function detail(state) {
    return {
        as_of_seq: state.seq,
        operation: summary(state),
        trace: { ...state.trace, as_of_seq: state.seq },
        executions: executions(state),
    };
}

/** `GET /operations`. */
export function page(state, status) {
    const row = summary(state);
    const items = status && status !== row.status ? [] : [row];
    return { items, next_cursor: null, as_of_seq: state.seq };
}

/** `GET /operations/{id}/steps/{step}`. */
export function stepDetail(state, id) {
    const index = state.trace.steps.findIndex((step) => step.id === id);
    if (index < 0) {
        return undefined;
    }
    const step = state.trace.steps[index];
    return {
        id: step.id,
        operation_id: state.trace.operation.id,
        ordinal: index + 1,
        kind: step.kind,
        phase: step.phase,
        parent: step.parent ?? null,
        cycle: step.cycle,
        outcome: step.outcome,
        started_at: step.started_at ?? null,
        ended_at: step.ended_at ?? null,
        summary: step.summary ?? null,
        reason: null,
        output: "plex exited 3 times in the last hour",
        intent: { kind: step.kind, note: "stored intent" },
        created_at: T0,
    };
}

/** The `nas` host, with the id the operation pins. */
export function hosts(state) {
    return [
        {
            id: state.hostId,
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
        },
    ];
}

/** One legacy action, so the list shows a row tagged Legacy. */
export function actions(state) {
    return [
        {
            id: "act-legacy",
            incident_id: "inc-legacy",
            host_id: state.hostId,
            skill: "restart_container",
            risk: "write",
            args: { name: "web" },
            status: "proposed",
            plan_hash: "9f2c1a4b".repeat(8),
            plan_hash_v1: "1a4b9f2c".repeat(8),
            plan_binding: "v1",
            proposed_by: "system:triage",
            expires_at: "2099-01-01T00:00:00Z",
            created_at: "2026-10-30T12:00:00Z",
            updated_at: "2026-10-30T12:00:00Z",
            legacy: true,
        },
    ];
}

/** `GET /overview`: nothing going on. */
export function overview(state) {
    return {
        incidents_action_needed: 0,
        incidents_open: 0,
        incident_groups: { needs_action: 0, monitor: 0, pending: 0, expected: 0, handled: 0 },
        approvals_waiting: 0,
        hosts_offline: 0,
        hosts_total: 1,
        as_of_seq: state.seq,
    };
}

/** `GET /settings`, read-only. */
export function settings() {
    return {
        hub: {
            name: "home",
            timezone: "UTC",
            bind: "127.0.0.1:7420",
            public_url: null,
            trusted_proxies: [],
        },
        llm_primary: null,
        llm_secondary: null,
        triage: {
            enabled: false,
            concurrency: 2,
            per_hour_runs: 6,
            reserved_for_incidents: 2,
            monthly_budget_usd: null,
        },
        discord: null,
        webhooks: [],
        heartbeat: { configured: false, url: "[unset]", interval: null, failing: false },
        usage: { tokens_in: 0, tokens_out: 0, cost_usd: 0 },
        editable: null,
    };
}
