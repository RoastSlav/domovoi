import { Link } from "react-router";
import type { Action, Execution } from "../../api/types";
import {
  Card,
  Cluster,
  InfoTip,
  InlineMessage,
  Progress,
  RelativeTime,
  Stack,
  StatusPill,
  Tag,
  Text,
} from "../../components";
import type { Tone } from "../../design/status";
import { riskMeta } from "../../design/tone";
import { ActionArgs } from "./ActionArgs";
import { Expiry, suggestedBy } from "./approvalBits";
import { planFromAction, planFromExecution, type ApprovalPlan } from "./approvalPlan";
import { PlanTag } from "./PlanTag";
import { StepApproveControl } from "./StepApproveControl";

/** Where the card's plan comes from. */
export type ApprovalSource =
  { kind: "step"; operationId: string; execution: Execution } | { kind: "legacy"; action: Action };

/** Data later surfaces supply; the card renders only what is present. */
export interface ApprovalExtras {
  effect?: { class: string; reversible: boolean | null };
  policy?: { outcome: string; rule?: string };
  refusal?: { reason: string };
}

/** What the card needs from its caller. */
export interface ApprovalCardProps {
  source: ApprovalSource;
  /** The host's display name: "on nas". */
  host?: string | undefined;
  /** Model text, shown secondary as "Model's reason (not checked)". */
  why?: string | undefined;
  extras?: ApprovalExtras | undefined;
  /** How Approve is drawn; destructive and unknown risk are always danger. */
  emphasis?: "primary" | "secondary";
  /** No Approve or Deny: for a place that shows the card without deciding. A legacy action never has them. */
  readOnly?: boolean;
  /** Adds a link to the operation, for lists outside its page. */
  operation?: { id: string; goal: string };
  /** Legacy only: adds a link to the incident. */
  incident?: { id: string; title?: string };
}

const NOTE_TONES: Record<string, Tone> = {
  failed: "danger",
  rejected: "danger",
  verify_failed: "danger",
  refused: "danger",
  denied: "danger",
  outcome_unknown: "warning",
  unavailable: "warning",
  not_run: "neutral",
};

/** What the state means for the person, where the pill alone does not say enough. */
function StateNote({ plan }: { plan: ApprovalPlan }) {
  if (plan.inFlight) {
    const sending = plan.state === "approved" || plan.state === "authorized";
    return (
      <Progress
        label={
          sending
            ? "Approved. Sending it to the agent…"
            : `Running on ${plan.host ?? "the host"}. Waiting for the result…`
        }
      />
    );
  }
  if (plan.state === "expired") {
    return (
      <InlineMessage tone="neutral" title="Nobody decided in time, so it expired">
        Nothing ran. Triage can propose it again.
      </InlineMessage>
    );
  }
  const tone = NOTE_TONES[plan.state];
  return tone ? (
    <InlineMessage tone={tone} title={plan.stateMeta.hint}>
      {plan.reason}
    </InlineMessage>
  ) : null;
}

function Extras({ extras }: { extras: ApprovalExtras }) {
  const { effect, policy, refusal } = extras;
  return (
    <>
      {effect && (
        <Cluster>
          <Tag>Effect: {effect.class}</Tag>
          {effect.reversible !== null && (
            <Tag>{effect.reversible ? "Reversible" : "Not reversible"}</Tag>
          )}
        </Cluster>
      )}
      {policy && (
        <Text size="sm" block>
          Policy: {policy.outcome}
          {policy.rule ? ` (rule ${policy.rule})` : ""}
        </Text>
      )}
      {refusal && (
        <InlineMessage tone="warning" title="The agent refused it">
          {refusal.reason}
        </InlineMessage>
      )}
    </>
  );
}

/**
 * The one approval card: contract, host, target, arguments, risk, expiry, the plan code and
 * its binding, then the decision. Model text is secondary and labelled. It serves operation
 * steps and, read-only, legacy actions; it never approves anything by itself.
 */
export function ApprovalCard({
  source,
  host,
  why,
  extras,
  emphasis = "primary",
  readOnly = false,
  operation,
  incident,
}: ApprovalCardProps) {
  const plan =
    source.kind === "step"
      ? planFromExecution(source.execution, host)
      : planFromAction(source.action, host);
  const risk = riskMeta(plan.risk);
  const decide = plan.waiting && !readOnly;

  return (
    <Card tone={plan.stateMeta.tone} label={`Action ${plan.skill}`}>
      <Stack gap={3}>
        <Cluster justify="between" align="start">
          <Cluster>
            <Text mono strong>
              {plan.skill}
            </Text>
            {host && (
              <Text muted size="sm">
                on {host}
              </Text>
            )}
          </Cluster>
          <Cluster>
            <StatusPill meta={risk} />
            <InfoTip title={`${risk.label} risk`} learnMore="concepts/actions.html">
              {risk.hint}
            </InfoTip>
            {source.kind === "legacy" && <Tag>Legacy</Tag>}
            {!plan.waiting && <StatusPill meta={plan.stateMeta} />}
          </Cluster>
        </Cluster>

        {(plan.check || plan.target) && (
          <Cluster>
            {plan.check && <Tag>Check {plan.check}</Tag>}
            {plan.target && <Tag>Target {plan.target}</Tag>}
          </Cluster>
        )}
        <ActionArgs args={plan.args} />
        {why && (
          <Text block>
            <Text muted>Model's reason (not checked): </Text>
            {why}
          </Text>
        )}
        {extras && <Extras extras={extras} />}

        <Cluster>
          {plan.suggestedBy && plan.createdAt && (
            <Text muted size="sm">
              {suggestedBy(plan.suggestedBy)} <RelativeTime at={plan.createdAt} />
            </Text>
          )}
          {plan.approvedBy && (
            <Text muted size="sm">
              Approved by {plan.approvedBy}
              {plan.approvedAt && (
                <>
                  {" "}
                  <RelativeTime at={plan.approvedAt} />
                </>
              )}
            </Text>
          )}
          {plan.waiting && <Expiry until={plan.expiresAt} />}
          <PlanTag planHash={plan.planHash} binding={plan.planBinding} />
          {operation && <Link to={`/operations/${operation.id}`}>{operation.goal}</Link>}
          {incident && plan.family === "action" && (
            <Link to={`/incidents/${incident.id}`}>{incident.title ?? "View incident"}</Link>
          )}
        </Cluster>

        <StateNote plan={plan} />
        {decide && source.kind === "step" && (
          <StepApproveControl plan={plan} operationId={source.operationId} emphasis={emphasis} />
        )}
      </Stack>
    </Card>
  );
}
