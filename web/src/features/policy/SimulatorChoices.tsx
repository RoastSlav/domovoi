import type { ReactNode } from "react";
import { useId } from "react";
import { Text } from "../../components";
import styles from "./simulator.module.css";

/** One choice in a `SimulatorChoices` group. */
export interface SimulatorChoice {
  disabled?: boolean;
  /** One line under the label saying what the choice means. */
  hint?: string;
  label: string;
  value: string;
}

/**
 * A few exclusive choices, all visible at once, each a tappable card that wraps onto the next
 * line when the room is short. The legend says what is being chosen and whether it is needed.
 * @param legend What is being chosen.
 * @param required Marks the choice as one the simulation cannot run without.
 * @param choices The choices; a disabled one stays listed so its absence is explained.
 * @param value The chosen value; empty when none is chosen.
 * @param onChange Receives the value the person picked.
 * @param hint One line under the group.
 */
export function SimulatorChoices({
  legend,
  required = false,
  choices,
  value,
  onChange,
  hint,
}: {
  choices: readonly SimulatorChoice[];
  hint?: ReactNode;
  legend: string;
  onChange: (value: string) => void;
  required?: boolean;
  value: string;
}) {
  const name = useId();
  return (
    <fieldset className={styles.choices}>
      <legend className={styles.legend}>
        {legend}
        {required && <span className={styles.cue}>(required)</span>}
      </legend>
      <div className={styles.cards}>
        {choices.map((choice) => (
          <label
            key={choice.value}
            className={styles.card}
            data-checked={choice.value === value || undefined}
            data-disabled={choice.disabled || undefined}
          >
            <input
              type="radio"
              name={name}
              className="visually-hidden"
              value={choice.value}
              checked={choice.value === value}
              // eslint-disable-next-line domovoi/disabled-needs-reason -- the card's hint says why
              disabled={choice.disabled}
              onChange={() => onChange(choice.value)}
            />
            <span className={styles.cardLabel}>{choice.label}</span>
            {choice.hint && (
              <Text muted size="sm">
                {choice.hint}
              </Text>
            )}
          </label>
        ))}
      </div>
      {hint && (
        <Text muted size="sm" block>
          {hint}
        </Text>
      )}
    </fieldset>
  );
}
