import { Link } from "react-router";
import { useOperationStep } from "../../api/hooks/operations";
import type { OperationTrace, TraceStep } from "../../api/operationTrace";
import type { Execution } from "../../api/types";
import {
  Button,
  CopyBlock,
  InlineMessage,
  KeyValueList,
  LoadError,
  RelativeTime,
  Skeleton,
  StatusPill,
  Text,
  DetailsScope,
  Visible,
} from "../../components";
import { Icon } from "../../design/icons";
import { stepKindMeta, stepOutcomeMeta } from "../../design/tone";
import { ApprovalCard } from "../approvals/ApprovalCard";
import { connections } from "./steps";
import { PROBLEM_OUTCOMES } from "./stepVisibility";
import styles from "./operations.module.css";

function duration(step: TraceStep): string | undefined {
  if (!step.started_at || !step.ended_at) {
    return undefined;
  }
  const seconds = Math.max(
    0,
    Math.round((Date.parse(step.ended_at) - Date.parse(step.started_at)) / 1000),
  );
  return seconds < 60 ? `${seconds} s` : `${Math.floor(seconds / 60)} min ${seconds % 60} s`;
}

/** The step's kind, outcome, phase, cycle and times, then its summary and reason. */
export function StepFacts({ step }: { step: TraceStep }) {
  const kind = stepKindMeta(step.kind);
  const outcome = stepOutcomeMeta(step.outcome);
  const took = duration(step);
  return (
    <div className={styles.inspectorBlock}>
      <Visible category="P1" id="inspector.kind">
        <Text strong>
          <Icon name={kind.icon} size={16} /> {kind.label}
        </Text>
      </Visible>
      <Visible category="P0" id="inspector.outcome">
        <span>
          <StatusPill meta={outcome} />
        </span>
      </Visible>
      <DetailsScope>
        <Visible category="P3" id="inspector.times" label="Times and phase" count={2}>
          <Text muted size="sm">
            Phase {step.phase}, cycle {step.cycle}
          </Text>
          <Text muted size="sm">
            Started {step.started_at ? <RelativeTime at={step.started_at} /> : "–"}. Ended{" "}
            {step.ended_at ? <RelativeTime at={step.ended_at} /> : "–"}.
            {took ? ` Took ${took}.` : ""}
          </Text>
        </Visible>
      </DetailsScope>
      {step.summary && (
        <Visible category="P1" id="inspector.summary">
          <Text block>{step.summary}</Text>
        </Visible>
      )}
    </div>
  );
}

/** Why a step ended as it did, for the outcomes that need an explanation. */
export function StepReason({ reason, outcome }: { reason: string | null; outcome: string }) {
  if (!reason || !PROBLEM_OUTCOMES.has(outcome)) {
    return null;
  }
  return (
    <Visible category="P0" id="inspector.reason">
      <InlineMessage tone={stepOutcomeMeta(outcome).tone}>{reason}</InlineMessage>
    </Visible>
  );
}

/** The action's approval card, read-only: the decision lives on the page, in "Waiting for you". */
export function StepAction({
  operationId,
  execution,
  hostNames,
  onReview,
}: {
  operationId: string;
  execution: Execution | undefined;
  hostNames: Map<string, string>;
  onReview: () => void;
}) {
  if (!execution) {
    return <Text muted>No action details are stored for this step.</Text>;
  }
  return (
    <Visible category="P0" id="inspector.approval">
      <div className={styles.inspectorBlock}>
        <ApprovalCard
          source={{ kind: "step", operationId, execution }}
          host={hostNames.get(execution.host_id)}
          readOnly
        />
        {execution.state === "awaiting_approval" && (
          <>
            <Text muted>Decide in Waiting for you on the page.</Text>
            <span>
              <Button size="sm" onClick={onReview}>
                Go to Waiting for you
              </Button>
            </span>
          </>
        )}
      </div>
    </Visible>
  );
}

/** Links to the steps this one is connected to, with the edge's words. */
export function StepConnections({
  trace,
  stepId,
  onSelect,
}: {
  trace: OperationTrace;
  stepId: string;
  onSelect: (id: string, opener: HTMLElement) => void;
}) {
  const links = connections(trace, stepId);
  return (
    <Visible category="P2" id="inspector.connections" label="Connections" count={links.length}>
      <div className={styles.inspectorBlock}>
        <Text strong>Connections</Text>
        {links.length === 0 && <Text muted>No connections</Text>}
        <ul>
          {links.map(({ edge, label, other }) => (
            <li key={`${edge.from}-${edge.to}-${edge.kind}`}>
              <Link
                to={{ search: `?step=${other.id}` }}
                replace
                onClick={(event) => {
                  event.preventDefault();
                  onSelect(other.id, event.currentTarget);
                }}
              >
                {label}: {stepKindMeta(other.kind).label}
                {other.summary ? `, ${other.summary}` : ""}
              </Link>
            </li>
          ))}
        </ul>
      </div>
    </Visible>
  );
}

function intentItems(intent: unknown) {
  if (typeof intent !== "object" || intent === null) {
    return [];
  }
  return Object.entries(intent as Record<string, unknown>)
    .filter(([key]) => key !== "kind")
    .map(([key, value]) => {
      const text = typeof value === "string" ? value : JSON.stringify(value);
      return { label: key, value: text.length > 400 ? `${text.slice(0, 400)}…` : text };
    });
}

/** The stored intent and the bounded, redacted output, loaded when the step is chosen. */
export function StepDetails({ operationId, stepId }: { operationId: string; stepId: string }) {
  const detail = useOperationStep(operationId, stepId);
  if (detail.isError) {
    return <LoadError error={detail.error} context="load the step" onRetry={detail.refetch} />;
  }
  if (!detail.data) {
    return <Skeleton shape="rows" count={3} />;
  }
  const items = intentItems(detail.data.intent);
  return (
    <div className={styles.inspectorBlock}>
      <Text strong>Details</Text>
      {items.length > 0 && (
        <Visible category="P2" id="inspector.intent" label="Intent" count={items.length}>
          <KeyValueList mono items={items} />
        </Visible>
      )}
      <Visible category="P2" id="inspector.output" label="Step output">
        {detail.data.output ? (
          <>
            <CopyBlock label="Step output" text={detail.data.output} />
            <Text muted size="sm">
              Redacted and shortened to 16 KiB.
            </Text>
          </>
        ) : (
          <Text muted>
            No output is stored for this step. Raw outputs are removed after the retention period.
          </Text>
        )}
      </Visible>
    </div>
  );
}
