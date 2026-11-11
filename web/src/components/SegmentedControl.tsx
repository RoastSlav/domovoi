import { useId } from "react";
import { Icon, type IconName } from "../design/icons";
import styles from "./form.module.css";

/** One choice in a segmented control. */
export interface Segment<T extends string> {
  icon?: IconName;
  label: string;
  value: T;
}

/**
 * Two to four exclusive choices shown side by side, such as a theme or a view mode. A radio
 * group underneath, so arrow keys move between the choices.
 *
 * Use it when every choice fits in a word and seeing them all helps. More choices, or long
 * ones, go in a `Select`; filters over a list are `FilterChips`.
 */
export function SegmentedControl<T extends string>({
  label,
  options,
  value,
  onChange,
  stretch = false,
}: {
  /** Names the group for assistive technology; not shown. */
  label: string;
  onChange: (value: T) => void;
  options: readonly Segment<T>[];
  /** Fill the width of the container. */
  stretch?: boolean;
  value: T;
}) {
  const name = useId();
  return (
    <fieldset className={styles.segmented} data-stretch={stretch || undefined}>
      <legend className="visually-hidden">{label}</legend>
      {options.map((option) => (
        <label key={option.value} className={styles.segment} data-checked={option.value === value}>
          <input
            type="radio"
            name={name}
            className="visually-hidden"
            checked={option.value === value}
            onChange={() => onChange(option.value)}
          />
          {option.icon && <Icon name={option.icon} size={14} />}
          {option.label}
        </label>
      ))}
    </fieldset>
  );
}
