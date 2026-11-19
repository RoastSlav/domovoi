import { Link } from "react-router";
import type { TraceStep } from "../../api/operationTrace";
import type { Execution } from "../../api/types";
import { RelativeTime, StatusPill, Tag, Text } from "../../components";
import { Icon } from "../../design/icons";
import { stepKindMeta, stepOutcomeMeta } from "../../design/tone";
import styles from "./operations.module.css";

/**
 * One step as a row: kind, summary, outcome and time. The title is a link to `?step=<id>`,
 * stretched over the row so a click anywhere selects the step.
 */
export function StepRow({
  step,
  execution,
  selected,
  retry,
  onSelect,
}: {
  step: TraceStep;
  execution: Execution | undefined;
  selected: boolean;
  retry: boolean;
  onSelect: (id: string, opener: HTMLElement) => void;
}) {
  const kind = stepKindMeta(step.kind);
  const at = step.ended_at ?? step.started_at;
  return (
    <li className={styles.stepRow} data-selected={selected || undefined}>
      <span className={styles.stepKind}>
        <Icon name={kind.icon} size={16} />
        <Link
          to={{ search: `?step=${step.id}` }}
          replace
          className={styles.stepLink}
          aria-current={selected ? "true" : undefined}
          onClick={(event) => {
            event.preventDefault();
            onSelect(step.id, event.currentTarget);
          }}
        >
          {kind.label}
        </Link>
      </span>
      <span className={styles.stepSummary}>
        {step.summary && <Text>{step.summary}</Text>}
        {execution && (
          <Text mono muted>
            {execution.contract}
          </Text>
        )}
        {retry && <Tag>Retry</Tag>}
      </span>
      <span className={styles.stepOutcome}>
        <StatusPill meta={stepOutcomeMeta(step.outcome)} size="sm" />
        {step.outcome === "running" && <Icon name="loader" size={14} />}
        <Text muted size="sm">
          {at ? <RelativeTime at={at} /> : "–"}
        </Text>
      </span>
    </li>
  );
}
