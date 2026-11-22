import { useId, type KeyboardEvent } from "react";
import styles from "./scrubber.module.css";

/**
 * A position slider over whole numbers, for moving through a sequence such as the moments of
 * a replay. A native range input with a visible label, so the browser's drag, touch and
 * keyboard behaviour is kept; Arrow keys move by one, Page keys by a tenth of the range, Home
 * and End to the ends.
 *
 * Use it to pick a position in an ordered list. For a quantity, use `NumberInput`.
 */
export function Scrubber({
  label,
  value,
  min,
  max,
  onChange,
  valueText,
  disabled = false,
  disabledReason,
}: {
  disabled?: boolean;
  /** Shown under the slider while it is disabled; say why it cannot be used. */
  disabledReason?: string;
  /** Visible label and accessible name. */
  label: string;
  max: number;
  min: number;
  onChange: (value: number) => void;
  /** Said by assistive technology instead of the bare number. */
  valueText: string;
  value: number;
}) {
  const id = useId();
  const reasonId = `${id}-reason`;
  const showReason = disabled && Boolean(disabledReason);
  const page = Math.max(1, Math.round((max - min) / 10));
  const clamp = (n: number): number => Math.min(max, Math.max(min, n));

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>): void => {
    if (event.altKey || event.ctrlKey || event.metaKey) {
      return;
    }
    const targets: Record<string, number> = {
      ArrowLeft: value - 1,
      ArrowDown: value - 1,
      ArrowRight: value + 1,
      ArrowUp: value + 1,
      PageDown: value - page,
      PageUp: value + page,
      Home: min,
      End: max,
    };
    const target = targets[event.key];
    if (target !== undefined) {
      event.preventDefault();
      if (clamp(target) !== value) {
        onChange(clamp(target));
      }
    }
  };

  return (
    <div className={styles.scrubber}>
      <label className={styles.label} htmlFor={id}>
        {label}
      </label>
      <input
        id={id}
        className={styles.range}
        type="range"
        min={min}
        max={max}
        step={1}
        value={value}
        disabled={disabled}
        aria-valuetext={valueText}
        aria-describedby={showReason ? reasonId : undefined}
        onChange={(event) => onChange(Number(event.target.value))}
        onKeyDown={onKeyDown}
      />
      {showReason && (
        <p className={styles.reason} id={reasonId}>
          {disabledReason}
        </p>
      )}
    </div>
  );
}
