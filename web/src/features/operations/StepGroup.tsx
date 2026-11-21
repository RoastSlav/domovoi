import type { TraceStep } from "../../api/operationTrace";
import { Disclosure } from "../../components";
import { stepKindMeta } from "../../design/tone";
import type { ReactNode } from "react";
import styles from "./operations.module.css";

/** "Model turn ×2, Evidence": the kinds in a run, in the order they first appear. */
export function kindSummary(steps: readonly TraceStep[]): string {
  const counts = new Map<string, number>();
  for (const step of steps) {
    counts.set(step.kind, (counts.get(step.kind) ?? 0) + 1);
  }
  return [...counts.entries()]
    .map(([kind, count]) => `${stepKindMeta(kind).label}${count > 1 ? ` ×${count}` : ""}`)
    .join(", ");
}

/**
 * A run of consecutive technical steps a detail level folds into one list item. It opens by
 * itself when `reveal` is true, so the selected step is never left behind a closed run.
 */
export function StepGroup({
  steps,
  reveal,
  children,
}: {
  children: ReactNode;
  reveal: boolean;
  steps: readonly TraceStep[];
}) {
  const count = steps.length;
  return (
    <Disclosure
      as="li"
      label={`${count} technical ${count === 1 ? "step" : "steps"}`}
      summary={kindSummary(steps)}
      reveal={reveal}
    >
      <ol
        className={styles.steps}
        aria-label={`${count} technical ${count === 1 ? "step" : "steps"}`}
      >
        {children}
      </ol>
    </Disclosure>
  );
}
