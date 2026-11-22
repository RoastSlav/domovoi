import type { OperationTrace, TraceEdge, TraceStep } from "../api/operationTrace";
import verifiedSuccess from "./fixtures/operation-trace/verified-success.json";

const KINDS = [
  "evidence_request",
  "reasoning_turn",
  "assessment",
  "plan",
  "policy_decision",
  "approval",
  "execution",
  "verification",
] as const;

/** A small seeded random number generator (mulberry32): the same seed gives the same numbers. */
function mulberry32(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/**
 * A valid trace of `n` steps `s0..s{n-1}`, deterministic for a `seed`. Kinds cycle through eight
 * kinds, `cycle` is `floor(i / 40)`, every step but `s0` depends on its predecessor with
 * probability 0.8 and is caused by one of the previous six with probability 0.5, every 11th
 * execution is a retry of the one before, and no edge points to a higher ordinal. Outcomes are
 * `succeeded` except every 17th step `failed`, every 23rd `pending` and the last `running`
 * (the last wins).
 */
export function makeTrace(n: number, seed: number): OperationTrace {
  const random = mulberry32(seed);
  const steps: TraceStep[] = [];
  const edges: TraceEdge[] = [];
  const seen = new Set<string>();
  const link = (from: number, to: number, kind: string): void => {
    const key = `${from}>${to}:${kind}`;
    if (to >= 0 && to < from && !seen.has(key)) {
      seen.add(key);
      edges.push({ from: `s${from}`, to: `s${to}`, kind });
    }
  };
  let lastExecution = -1;
  let executions = 0;
  for (let i = 0; i < n; i += 1) {
    const kind = KINDS[i % KINDS.length] as string;
    let outcome = "succeeded";
    if (i === n - 1) {
      outcome = "running";
    } else if ((i + 1) % 17 === 0) {
      outcome = "failed";
    } else if ((i + 1) % 23 === 0) {
      outcome = "pending";
    }
    steps.push({
      id: `s${i}`,
      parent: null,
      kind,
      phase: "diagnose",
      cycle: Math.floor(i / 40),
      outcome,
      started_at: null,
      ended_at: null,
      summary: `Step ${i}`,
      refs: [],
    });
    if (i > 0) {
      if (random() < 0.8) {
        link(i, i - 1, "depends_on");
      }
      if (random() < 0.5) {
        link(i, Math.max(0, i - 6) + Math.floor(random() * Math.min(6, i)), "caused_by");
      }
    }
    if (kind === "execution") {
      executions += 1;
      if (executions % 11 === 0 && lastExecution >= 0) {
        link(i, lastExecution, "retry_of");
      }
      lastExecution = i;
    }
  }
  return {
    schema_version: 1,
    as_of_seq: 1,
    operation: { ...(verifiedSuccess.operation as OperationTrace["operation"]), status: "running" },
    steps,
    edges,
  };
}
