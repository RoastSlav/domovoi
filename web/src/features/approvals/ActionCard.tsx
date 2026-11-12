import { Link } from "react-router";
import type { Action } from "../../api/types";
import {
  Card,
  Cluster,
  Countdown,
  InfoTip,
  InlineMessage,
  Progress,
  RelativeTime,
  Stack,
  StatusPill,
  Tag,
  Text,
  useRemaining,
} from "../../components";
import { actionStatusMeta, riskMeta } from "../../design/tone";
import { ActionArgs } from "./ActionArgs";
import { ApproveControl } from "./ApproveControl";

/** Under this much time left, the countdown of a waiting action turns to a warning. */
export const EXPIRY_WARNING_MS = 15 * 60 * 1000;

const IN_FLIGHT = new Set(["approved", "dispatched"]);

/** Who suggested the action, as a sentence. The hub sends `system:triage` or `user:<id>`. */
export function suggestedBy(proposedBy: string): string {
  switch (proposedBy) {
    case "system:triage":
      return "Suggested by triage";
    case "system:chat":
      return "Suggested in chat";
    default:
      return proposedBy.startsWith("user:") ? "Suggested by a person" : "Suggested by Domovoi";
  }
}

/** The time left to decide, as a tag that turns to a warning near the end. */
export function Expiry({ until }: { until: string }) {
  const remaining = useRemaining(until);
  if (remaining === 0) {
    return <Tag>Expired</Tag>;
  }
  return (
    <Tag {...(remaining < EXPIRY_WARNING_MS ? { tone: "warning" as const } : {})}>
      Expires in <Countdown remaining={remaining} until={until} />
    </Tag>
  );
}

/** What the action's state means for the person, where the pill alone does not say enough. */
function StateNote({ action, host }: { action: Action; host: string | undefined }) {
  if (IN_FLIGHT.has(action.status)) {
    return (
      <Progress
        label={
          action.status === "approved"
            ? "Approved. Sending it to the agent…"
            : `Running on ${host ?? "the host"}. Waiting for the result…`
        }
      />
    );
  }
  switch (action.status) {
    case "expired":
      return (
        <InlineMessage tone="neutral" title="Nobody decided in time, so it expired">
          Nothing ran. Triage can propose it again.
        </InlineMessage>
      );
    case "failed":
    case "rejected":
    case "verify_failed":
      return (
        <InlineMessage tone="danger" title={actionStatusMeta(action.status).hint}>
          {action.reason}
        </InlineMessage>
      );
    default:
      return null;
  }
}

/**
 * One action, the same on the incident page, the Approvals page and in a chat: the skill,
 * where it runs, its risk, its arguments, why the model wants it, who suggested it and the
 * time left; while it waits, Approve and Deny. A waiting action's status is the section it
 * sits in, so only a decided or running one draws a status pill.
 */
export function ActionCard({
  action,
  host,
  why,
  showIncident = false,
  incidentTitle,
  approve = "primary",
}: {
  action: Action;
  /**
   * How Approve is drawn: `primary` where it is the card's main action (the Approvals
   * page), `secondary` where the page already has a primary button. A destructive action's
   * Approve is always a danger button.
   */
  approve?: "primary" | "secondary";
  /** The host's name; the card says "on nas" when it is known. */
  host?: string | undefined;
  /** The incident's title, as the text of the link to it; "View incident" without it. */
  incidentTitle?: string | undefined;
  /** Links to the incident: for lists that are not the incident's own page. */
  showIncident?: boolean;
  /** Why the model wants it, where the page has the report that says so. */
  why?: string | undefined;
}) {
  const status = actionStatusMeta(action.status);
  const risk = riskMeta(action.risk);
  const waiting = action.status === "proposed";

  return (
    <Card tone={status.tone} label={`Action ${action.skill}`}>
      <Stack gap={3}>
        <Cluster justify="between" align="start">
          <Cluster>
            <Text mono strong>
              {action.skill}
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
            {!waiting && <StatusPill meta={status} />}
          </Cluster>
        </Cluster>

        <ActionArgs args={action.args} />
        {why && (
          <Text block>
            <Text muted>Why: </Text>
            {why}
          </Text>
        )}

        <Cluster>
          <Text muted size="sm">
            {suggestedBy(action.proposed_by)} <RelativeTime at={action.created_at} />
          </Text>
          {waiting && <Expiry until={action.expires_at} />}
          {showIncident && (
            <Link to={`/incidents/${action.incident_id}`}>{incidentTitle ?? "View incident"}</Link>
          )}
        </Cluster>

        <StateNote action={action} host={host} />
        {waiting && <ApproveControl action={action} host={host} emphasis={approve} />}
      </Stack>
    </Card>
  );
}
