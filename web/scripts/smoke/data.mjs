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

const fixtureRoot = join(dirname(fileURLToPath(import.meta.url)), "../../src/test/fixtures");

/** A knowledge fixture, with its host id replaced by the id the mock hub gives `nas`. */
export function knowledgeFixture(name, hostId) {
    const text = readFileSync(join(fixtureRoot, "knowledge", `${name}.json`), "utf8");
    return JSON.parse(text.replaceAll("host-nas", hostId));
}

/** An experience fixture, read fresh so the smoke test and vitest share one contract. */
export function experienceFixture(name) {
    return JSON.parse(readFileSync(join(fixtureRoot, "experience", `${name}.json`), "utf8"));
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
        facts: knowledgeFixture("facts-page", pinned.host).items,
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
        retention: {
            entries: [
                ["signals_days", 30, null],
                ["incident_timeline_days", 90, null],
                ["conversations_days", 180, null],
                ["notifications_days", 30, null],
                ["triage_cache_days", 90, null],
                ["resolved_advisories_days", 30, null],
                ["operation_journal_days", 180, "operations exist"],
                ["execution_outputs_days", 30, "execution outputs are stored apart"],
                ["retracted_facts_days", 30, "knowledge exists"],
            ].map(([key, days, once]) => ({
                key,
                days,
                default_days: days,
                applies: once ? "later" : "now",
                applies_once: once,
            })),
            backups_keep: 7,
            backups_keep_pre_migrate: 3,
        },
        editable: null,
    };
}

const HASH = "9f2c".repeat(16);

/** `GET /audit`: five chained and two legacy entries; one detail holds an angle bracket. */
export function auditPage(history) {
    const row = (seq, legacy) => ({
        seq,
        at: "2026-11-18T10:00:00Z",
        actor: "user:0190",
        kind: "action.approved",
        subject: `action:${seq}`,
        detail: seq === 7 ? "<b>marked</b>" : "{}",
        detail_truncated: false,
        history: legacy ? "legacy" : "chained",
        hash: legacy ? null : HASH,
    });
    const legacy = [row(2, true), row(1, true)];
    const chained = [7, 6, 5, 4, 3].map((seq) => row(seq, false));
    const items = history === "legacy" ? legacy : chained;
    return { legacy_last_id: 2, next_cursor: null, items };
}

/** `GET /audit/verification`: a recent check, or the whole log. */
export function auditVerification(scope) {
    return {
        scope: scope === "full" ? "full" : "recent",
        recent_limit: 10000,
        checked_at: "2026-11-18T10:00:00Z",
        legacy: {
            status: scope === "full" ? "verified" : "count_only",
            count: 2,
            expected_count: null,
            actual_count: null,
            digest_matches: null,
        },
        chained: {
            status: "intact",
            checked: 5,
            head_seq: 7,
            head_hash: HASH,
            first_broken_seq: null,
            break_reason: null,
            break_text: null,
        },
        anchor: {
            install_id: "install-1",
            legacy_last_id: 2,
            legacy_count: 2,
            legacy_digest: "ab".repeat(32),
            anchor_hash: "cd".repeat(32),
        },
        head: { seq: 7, hash: HASH },
    };
}
