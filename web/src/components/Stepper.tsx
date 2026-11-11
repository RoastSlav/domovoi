import { Icon } from "../design/icons";
import { stepStateMeta, type StepState } from "../design/tone";
import styles from "./auth.module.css";

/** One step of a `Stepper`. */
export interface StepperStep {
  id: string;
  label: string;
  state: StepState;
}

/**
 * The steps of a guided flow as an ordered list. Each shows its number, name, an icon and
 * the state in words, so colour is never the only signal; the current one is marked with
 * `aria-current="step"`.
 *
 * Use it for a short sequence a person walks through once (first-run setup). Do not use it
 * for tabs or for navigation between pages.
 */
export function Stepper({ steps, label }: { label: string; steps: StepperStep[] }) {
  return (
    <ol className={styles.stepper} aria-label={label}>
      {steps.map((step, index) => {
        const meta = stepStateMeta(step.state);
        return (
          <li
            key={step.id}
            className={styles.step}
            data-tone={step.state === "upcoming" ? undefined : meta.tone}
            aria-current={step.state === "current" ? "step" : undefined}
          >
            <Icon name={meta.icon} size={14} />
            <span className={styles.stepNumber}>{index + 1}.</span>
            {step.label}
            <span className="visually-hidden"> ({meta.label.toLowerCase()})</span>
          </li>
        );
      })}
    </ol>
  );
}
