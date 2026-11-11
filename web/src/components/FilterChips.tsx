import { useId } from "react";
import type { Tone } from "../design/tone";
import { Count } from "./Count";
import styles from "./form.module.css";

/** One filter chip. */
export interface Chip<T extends string> {
  /** How many items the filter matches, with what is counted: `{ value: 3, label: "open" }`. */
  count?: { label: string; value: number };
  label: string;
  /** The tone of the count; neutral unless given. */
  tone?: Tone;
  value: T;
}

interface Common<T extends string> {
  /** Names the group for assistive technology; not shown. */
  label: string;
  options: readonly Chip<T>[];
}

/**
 * Toggle chips that filter a list, with optional counts. Single choice is a radio group;
 * `multiple` makes each chip a checkbox.
 *
 * Use it for the few filters people switch between all the time (status, kind). Rare or
 * long filters go in a `Select`.
 */
export function FilterChips<T extends string>(
  props: Common<T> &
    (
      | { multiple?: false; onChange: (value: T) => void; value: T }
      | { multiple: true; onChange: (value: T[]) => void; value: readonly T[] }
    ),
) {
  const name = useId();
  const isOn = (value: T): boolean =>
    props.multiple ? props.value.includes(value) : props.value === value;
  const toggle = (value: T): void => {
    if (props.multiple) {
      props.onChange(
        isOn(value) ? props.value.filter((other) => other !== value) : [...props.value, value],
      );
    } else {
      props.onChange(value);
    }
  };
  return (
    <fieldset className={styles.chips}>
      <legend className="visually-hidden">{props.label}</legend>
      {props.options.map((option) => (
        <label key={option.value} className={styles.chip} data-checked={isOn(option.value)}>
          <input
            type={props.multiple ? "checkbox" : "radio"}
            name={name}
            className="visually-hidden"
            checked={isOn(option.value)}
            onChange={() => toggle(option.value)}
          />
          {option.label}
          {option.count && (
            <Count
              tone={option.tone ?? "neutral"}
              value={option.count.value}
              label={option.count.label}
            />
          )}
        </label>
      ))}
    </fieldset>
  );
}
