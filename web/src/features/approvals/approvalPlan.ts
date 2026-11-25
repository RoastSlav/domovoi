import type { Action, Execution } from "../../api/types";
import type { StatusMeta } from "../../design/status";
import { actionStatusMeta, executionStateMeta } from "../../design/tone";

/** Everything an approval card shows, whether it comes from an operation step or a legacy action. */
/** Who authorized an execution, as one line; `null` while nobody has. */
export function authorizationText(
  authorization: Execution["authorization"] | undefined,
): string | null {
  if (!authorization) return null;
  if (authorization.kind === "policy" && authorization.permit) {
    const { policy_version: version, rules } = authorization.permit;
    return `Allowed by policy version ${version}, rules ${rules.join(", ")}`;
  }
  if (authorization.kind === "human") {
    const where = authorization.channel === "web" ? " in the web UI" : "";
    return `Approved by a person${where}`;
  }
  return null;
}

export interface ApprovalPlan {
  /** Action id or step id (keys, aria). */
  id: string;
  family: "step" | "action";
  /** Display name: `skillName(contract)`. */
  skill: string;
  contract: string;
  contractVersion: number | null;
  host: string | undefined;
  check: string | null;
  target: string | null;
  args: Record<string, string>;
  /** `read`, `write`, `destructive` or `unknown` (a missing risk is unknown). */
  risk: string;
  /** Execution state (step) or action status (action). */
  state: string;
  stateMeta: StatusMeta;
  waiting: boolean;
  inFlight: boolean;
  planHash: string;
  planBinding: string;
  expiresAt: string;
  createdAt: string | null;
  suggestedBy: string | null;
  approvedBy: string | null;
  /** Who authorized it, in words: a person in the web UI, or a policy and its rules. */
  authorizedBy: string | null;
  approvedAt: string | null;
  reason: string | null;
  confirmRequired: boolean;
  /** What must be typed to approve: the skill's name, exactly as the hub compares it. */
  confirmText: string;
}

/** `skill.restart_container` becomes `restart_container`; anything else is unchanged. */
export function skillName(contract: string): string {
  return contract.startsWith("skill.") ? contract.slice("skill.".length) : contract;
}

/** The plan of an Execution step. A missing `confirm_required` is treated as required. */
export function planFromExecution(execution: Execution, host?: string): ApprovalPlan {
  const skill = skillName(execution.contract);
  return {
    id: execution.step,
    family: "step",
    skill,
    contract: execution.contract,
    contractVersion: execution.contract_version,
    host,
    check: execution.check,
    target: execution.target ?? null,
    args: execution.args,
    risk: execution.risk ?? "unknown",
    state: execution.state,
    stateMeta: executionStateMeta(execution.state),
    waiting: execution.state === "awaiting_approval",
    inFlight: execution.state === "authorized" || execution.state === "dispatched",
    planHash: execution.plan_hash,
    planBinding: execution.plan_binding,
    expiresAt: execution.expires_at,
    createdAt: null,
    suggestedBy: null,
    approvedBy: execution.approved_by ?? null,
    authorizedBy: authorizationText(execution.authorization),
    approvedAt: execution.approved_at ?? null,
    reason: execution.reason ?? null,
    confirmRequired: (execution.confirm_required as boolean | undefined) ?? true,
    confirmText: skill,
  };
}

/** The plan of a legacy action. */
export function planFromAction(action: Action, host?: string): ApprovalPlan {
  return {
    id: action.id,
    family: "action",
    skill: action.skill,
    contract: action.skill,
    contractVersion: null,
    host,
    check: null,
    target: null,
    args: action.args,
    risk: action.risk,
    state: action.status,
    stateMeta: actionStatusMeta(action.status),
    // A legacy action can no longer be decided; it is history.
    waiting: false,
    inFlight: action.status === "approved" || action.status === "dispatched",
    planHash: action.plan_hash,
    planBinding: action.plan_binding,
    expiresAt: action.expires_at,
    createdAt: action.created_at,
    suggestedBy: action.proposed_by,
    approvedBy: null,
    authorizedBy: null,
    approvedAt: null,
    reason: action.reason ?? null,
    confirmRequired: action.risk === "destructive",
    confirmText: action.skill,
  };
}
