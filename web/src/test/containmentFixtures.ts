import type {
  ContainmentBlock,
  ContainmentStatus,
  ContainmentStep,
  StepDetail,
} from "../api/types";

/** A host's containment as the hub reports it before anything happened: the gate not asked, no blocks. */
export function containmentStatus(overrides: Partial<ContainmentStatus> = {}): ContainmentStatus {
  return {
    available: true,
    blocks: [],
    drills: { state: "unknown" },
    gate: { state: "unknown" },
    ...overrides,
  };
}

/** One block the hub recorded, on documentation addresses only. */
export function block(overrides: Partial<ContainmentBlock> = {}): ContainmentBlock {
  return {
    assurance: "verified",
    authorization: "human",
    block_id: "b-1",
    created_at: "2026-11-01T10:00:00Z",
    host_id: "host-nas",
    operation_id: "op-1",
    phase: "confirmed",
    source: "203.0.113.7/32",
    step_id: "s-1",
    ttl_s: 3600,
    unprotected_paths: [],
    updated_at: "2026-11-01T10:00:30Z",
    ...overrides,
  };
}

/** A containment step on the step detail, verified unless said otherwise. */
export function containmentStep(overrides: Partial<ContainmentStep> = {}): ContainmentStep {
  return {
    assurance: "verified",
    phase: "proposed",
    source: "203.0.113.7/32",
    ttl_s: 3600,
    unprotected_paths: [],
    ...overrides,
  };
}

/** A step the hub could not verify: the reason and the paths it left uncovered. */
export function unverifiedStep(overrides: Partial<ContainmentStep> = {}): ContainmentStep {
  return containmentStep({
    assurance: "unverified",
    reason: "key_not_pinned",
    unprotected_paths: ["ssh on 192.0.2.10", "hub link from 198.51.100.4"],
    ...overrides,
  });
}

/** The step detail of a containment step, whose block is `containment` (none: a plain step). */
export function containmentStepDetail(containment: ContainmentStep | null): StepDetail {
  return {
    containment,
    created_at: "2026-11-01T10:00:00Z",
    cycle: 1,
    id: "step-exec-1",
    intent: {},
    kind: "execution",
    operation_id: "op-1",
    ordinal: 3,
    outcome: "pending",
    phase: "execute",
  };
}
