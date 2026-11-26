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

/**
 * A fresh, mutable state for one run: by default the waiting-approval operation; `trace` names another of the golden trace fixtures to serve instead.
 */
export function makeState({ destructive = false, trace: fixture = "waiting-approval" } = {}) {
    const trace = load(fixture);
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

const SECOND = 1000;
const iso = (ms) => new Date(ms).toISOString().replace(".000Z", "Z");
/** Execution outcomes that are also execution states of the same name in the journal. */
const EXECUTION_FINAL = ["succeeded", "failed", "refused", "outcome_unknown"];

/**
 * `GET /operations/{id}/history`: the journal rows an operation of these steps would have left,
 * as the hub projects them. A step is created when it started (a step with no time gets the
 * previous step's time plus one second); an execution step also records `awaiting_approval`, then
 * `dispatched` and its final state when it got that far. Always complete.
 */
export function history(state) {
    const { operation, steps } = state.trace;
    const rows = [{ at: Date.parse(operation.created_at), kind: "operation.created", step: null }];
    let previous = Date.parse(operation.created_at);
    for (const step of steps) {
        const created = step.started_at ? Date.parse(step.started_at) : previous + SECOND;
        previous = created;
        rows.push({ at: created, kind: "operation.step_created", step: step.id });
        if (step.kind !== "execution") {
            continue;
        }
        rows.push({
            at: created,
            kind: "operation.execution",
            step: step.id,
            to: "awaiting_approval",
        });
        if (step.outcome === "pending") {
            continue;
        }
        rows.push({
            at: created + SECOND,
            kind: "operation.execution",
            step: step.id,
            to: "dispatched",
        });
        if (EXECUTION_FINAL.includes(step.outcome)) {
            const ended = step.ended_at ? Date.parse(step.ended_at) : created + 2 * SECOND;
            rows.push({
                at: Math.max(ended, created + 2 * SECOND),
                kind: "operation.execution",
                step: step.id,
                to: step.outcome,
            });
        }
    }
    const ordered = rows
        .map((row, index) => ({ row, index }))
        .sort((a, b) => a.row.at - b.row.at || a.index - b.index)
        .map(({ row }, index) => ({
            seq: index + 1,
            at: iso(row.at),
            kind: row.kind,
            step: row.step,
            state: row.to ?? null,
        }));
    return {
        as_of_seq: state.seq,
        complete: true,
        incomplete_reason: null,
        events: ordered,
    };
}

/** Operations other than the main one, by status: a small different page for each. */
const OTHER_OPERATIONS = {
    running: {
        id: "0192a000-0000-7000-8000-00000000f001",
        goal: "Rebuild the media index",
        waiting_for: [],
        pause_reason: null,
    },
    waiting_user: {
        id: "0192a000-0000-7000-8000-00000000f002",
        goal: "Choose how long to keep snapshots",
        waiting_for: ["answer"],
        pause_reason: null,
    },
    paused: {
        id: "0192a000-0000-7000-8000-00000000f003",
        goal: "Prune old snapshots",
        waiting_for: [],
        pause_reason: { type: "manual" },
    },
};

/** `GET /operations`: the main operation when its status matches, plus the others of that status. */
export function page(state, status) {
    const row = summary(state);
    const other = status ? OTHER_OPERATIONS[status] : undefined;
    const items = [
        ...(!status || status === row.status ? [row] : []),
        ...(other ? [{ ...row, ...other, status, updated_at: "2026-10-31T11:00:00Z" }] : []),
    ];
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
        {
            id: "0192a000-0000-7000-8000-00000000c0aa",
            name: "old-agent",
            kind: "remote",
            connected: false,
            agent_enrolled: true,
            version: "0.0.0",
            outdated: false,
            protocol_state: "incompatible",
            reported_protocol: "0.9",
            hub_protocol: "1.1",
            reported_release: "0.0.0",
            incompatible_since: "2026-10-31T10:00:00Z",
            last_seen: "2026-10-31T10:00:00Z",
            open_incidents: 0,
        },
    ];
}

/** Incidents the mock knows, with the group each belongs to (not sent to the page). */
const INCIDENTS = [
    ["inc-plex", "plex keeps restarting", "critical", "needs_action", "2026-10-31T11:50:00Z"],
    ["inc-p1", "plex slow response 1", "warning", "monitor", "2026-10-31T11:40:00Z"],
    ["inc-p2", "plex slow response 2", "warning", "monitor", "2026-10-31T11:39:00Z"],
    ["inc-p3", "plex slow response 3", "warning", "monitor", "2026-10-31T11:38:00Z"],
    ["inc-p4", "plex slow response 4", "warning", "monitor", "2026-10-31T11:37:00Z"],
    ["inc-p5", "plex slow response 5", "warning", "monitor", "2026-10-31T11:36:00Z"],
    ["inc-old", "Plex old alert", "warning", "handled", "2026-10-30T09:00:00Z"],
    ["inc-disk", "nas disk is almost full", "warning", "monitor", "2026-10-31T09:00:00Z"],
];

/** `GET /incidents`: honours `group` and `q`, newest first, in one page. */
export function incidents(state, group, q) {
    const needle = (q ?? "").toLowerCase();
    const items = INCIDENTS.filter(
        ([, title, , own]) => (!group || own === group) && title.toLowerCase().includes(needle),
    ).map(([id, title, severity, , at]) => ({
        id,
        host_id: state.hostId,
        check: "container.running",
        target: "plex",
        title,
        status: "open",
        severity,
        verdict: null,
        opened_at: at,
        last_firing_at: at,
        occurrences: 1,
    }));
    return { items, next_cursor: null, as_of_seq: state.seq };
}

/** `GET /notes/search`: one note for any text, with its matching excerpt. */
export function noteHits(state, q) {
    return [
        {
            note: {
                id: "note-plex",
                title: "How plex is set up",
                body_markdown: `Plex runs in a container; the text asked for was ${q}.`,
                host_id: state.hostId,
                tags: [],
                updated_at: "2026-10-30T12:00:00Z",
            },
            score: 1,
            snippet: "Plex runs in a container.",
        },
    ];
}

/**
 * `GET /overview`: one of everything. The attention total is the sum the hub documents:
 * needs-action incidents, approvals, operations waiting for approval or an answer, paused
 * operations, unknown results and offline hosts (an incompatible host counts as offline).
 */
export function overview(state) {
    const waitingApproval = state.trace.operation.status === "waiting_approval" ? 1 : 0;
    const paused = 1 + (state.trace.operation.status === "paused" ? 1 : 0);
    const counts = {
        operations_running: 1 + (state.trace.operation.status === "running" ? 1 : 0),
        operations_waiting_approval: waitingApproval,
        operations_waiting_user: 1,
        operations_paused: paused,
        hosts_offline: 1,
    };
    const needsAction = 1;
    return {
        incidents_action_needed: needsAction,
        incidents_open: 8,
        incident_groups: {
            needs_action: needsAction,
            monitor: 6,
            pending: 0,
            expected: 0,
            handled: 1,
        },
        ...counts,
        attention_total:
            needsAction +
            counts.operations_waiting_approval +
            counts.operations_waiting_user +
            counts.operations_paused +
            counts.hosts_offline,
        hosts_total: 2,
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

/** `GET /audit`: five entries; one detail holds an angle bracket. */
export function auditPage() {
    const row = (seq) => ({
        seq,
        at: "2026-11-18T10:00:00Z",
        actor: "user:0190",
        kind: "action.approved",
        subject: `action:${seq}`,
        detail: seq === 7 ? "<b>marked</b>" : "{}",
        detail_truncated: false,
        hash: HASH,
    });
    return { next_cursor: null, items: [7, 6, 5, 4, 3].map(row) };
}

/** `GET /audit/verification`: a recent check, or the whole log. */
export function auditVerification(scope) {
    return {
        scope: scope === "full" ? "full" : "recent",
        recent_limit: 10000,
        checked_at: "2026-11-18T10:00:00Z",
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
            anchor_hash: "cd".repeat(32),
        },
        head: { seq: 7, hash: HASH },
    };
}
