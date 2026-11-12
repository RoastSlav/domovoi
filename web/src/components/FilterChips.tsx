import { useEffect, useId, useRef, useState, type UIEvent } from "react";
import type { Tone } from "../design/tone";
import { Count } from "./Count";
import styles from "./choice.module.css";

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
  const box = useRef<HTMLFieldSetElement>(null);
  const [edges, setEdges] = useState({ start: false, end: false });
  const selected = props.multiple ? props.value.join(",") : props.value;
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
  const measure = (node: HTMLElement | null): void => {
    if (!node) {
      return;
    }
    const start = node.scrollLeft > 1;
    const end = node.scrollLeft + node.clientWidth < node.scrollWidth - 1;
    setEdges((now) => (now.start === start && now.end === end ? now : { start, end }));
  };
  // On a phone the chips scroll sideways: keep the chosen one in view and fade the cut edges.
  useEffect(() => {
    const node = box.current;
    if (!node) {
      return;
    }
    const chip = node.querySelector<HTMLElement>('[data-checked="true"]');
    if (chip && node.scrollWidth > node.clientWidth) {
      node.scrollLeft = chip.offsetLeft - (node.clientWidth - chip.offsetWidth) / 2;
    }
    measure(node);
  }, [selected]);
  return (
    <fieldset
      ref={box}
      className={styles.chips}
      data-fade-start={edges.start || undefined}
      data-fade-end={edges.end || undefined}
      onScroll={(event: UIEvent<HTMLFieldSetElement>) => measure(event.currentTarget)}
      onFocus={(event) => {
        // jsdom has no layout, so the call may be missing.
        event.target.closest("label")?.scrollIntoView?.({ block: "nearest", inline: "nearest" });
      }}
    >
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
