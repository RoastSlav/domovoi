import { useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { Icon, type IconName } from "../design/icons";
import { Popover } from "./Popover";
import styles from "./menu.module.css";

/** One choice in an `OptionPicker`. */
export interface PickerOption {
  label: string;
  /** Shown right of the label, usually a `StatusPill` or a `Tag`. */
  meta?: ReactNode;
  value: string;
}

/** With more options than this, the list gets a box to type in. */
export const PICKER_FILTER_FROM = 8;

/**
 * A select whose options can carry a status: a button showing the current choice that opens
 * a list. Arrow keys move through the list, Enter chooses, Escape closes. With more than
 * eight options, typing filters them.
 *
 * The chosen option is marked with a check at the right of the list. The button can carry a leading
 * `icon`, a muted `caption` naming what is chosen and a `valueMeta` (a status dot, a count)
 * after the current choice; the caption is dropped on a phone.
 *
 * Use it where the options need a pill or tag beside them (the host selector). Do not use it
 * for plain choices in a form: that is `Select` inside a `Field`.
 */
export function OptionPicker({
  label,
  value,
  options,
  onChange,
  icon,
  caption,
  valueMeta,
}: {
  /** A muted word before the current choice, like "Host". Hidden on a phone. */
  caption?: string;
  /** A leading icon on the button. */
  icon?: IconName;
  /** What is being chosen; the control's accessible name. */
  label: string;
  onChange: (value: string) => void;
  options: PickerOption[];
  value: string;
  /** Shown on the button right of the current choice: a status dot, a count. */
  valueMeta?: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const [filter, setFilter] = useState("");
  const trigger = useRef<HTMLButtonElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const search = useRef<HTMLInputElement>(null);
  const id = useId();
  const searchable = options.length > PICKER_FILTER_FROM;
  const wanted = filter.trim().toLowerCase();
  const shown = wanted ? options.filter((o) => o.label.toLowerCase().includes(wanted)) : options;
  const current = options.find((option) => option.value === value);

  const items = (): HTMLElement[] => [
    ...(list.current?.querySelectorAll<HTMLElement>('[role="option"]') ?? []),
  ];

  useEffect(() => {
    if (!open) {
      return;
    }
    if (search.current) {
      search.current.focus();
      return;
    }
    const all = [...(list.current?.querySelectorAll<HTMLElement>('[role="option"]') ?? [])];
    (all.find((item) => item.getAttribute("aria-selected") === "true") ?? all[0])?.focus();
  }, [open]);

  const close = (): void => {
    setOpen(false);
    setFilter("");
  };
  const choose = (next: string): void => {
    close();
    trigger.current?.focus();
    if (next !== value) {
      onChange(next);
    }
  };

  const onKeyDown = (event: KeyboardEvent): void => {
    const all = items();
    const at = all.indexOf(document.activeElement as HTMLElement);
    const typing = document.activeElement === search.current;
    if (event.key === "Enter" && typing) {
      event.preventDefault();
      const first = shown[0];
      if (first) {
        choose(first.value);
      }
      return;
    }
    const to =
      event.key === "ArrowDown"
        ? Math.min(at + 1, all.length - 1)
        : event.key === "ArrowUp"
          ? at - 1
          : event.key === "Home" && !typing
            ? 0
            : event.key === "End" && !typing
              ? all.length - 1
              : undefined;
    if (to === undefined) {
      return;
    }
    event.preventDefault();
    if (to < 0) {
      // Up from the first option goes back to the filter box, if there is one.
      search.current?.focus();
    } else {
      all[to]?.focus();
    }
  };

  return (
    <>
      <button
        ref={trigger}
        type="button"
        role="combobox"
        className={styles.pickerTrigger}
        aria-label={label}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={open ? id : undefined}
        onClick={() => (open ? close() : setOpen(true))}
      >
        {icon && (
          <span className={styles.pickerIcon}>
            <Icon name={icon} />
          </span>
        )}
        {caption && <span className={styles.pickerCaption}>{caption}</span>}
        <span className={styles.pickerValue}>{current?.label ?? value}</span>
        {valueMeta && <span className={styles.pickerMeta}>{valueMeta}</span>}
        <Icon name="chevron-down" size={14} className={styles.pickerChevron ?? ""} />
      </button>
      {open && (
        <Popover anchor={trigger} label={label} onClose={close} flush>
          <div className={styles.picker} onKeyDown={onKeyDown}>
            {searchable && (
              <input
                ref={search}
                type="search"
                className={styles.pickerSearch}
                aria-label={`Filter: ${label}`}
                placeholder="Type to filter"
                value={filter}
                onChange={(event) => setFilter(event.target.value)}
              />
            )}
            {shown.length > 0 ? (
              <div
                ref={list}
                id={id}
                role="listbox"
                aria-label={label}
                className={styles.pickerList}
              >
                {shown.map((option) => (
                  <button
                    key={option.value}
                    type="button"
                    role="option"
                    tabIndex={-1}
                    aria-selected={option.value === value}
                    className={styles.pickerOption}
                    onClick={() => choose(option.value)}
                  >
                    <span className={styles.pickerValue}>{option.label}</span>
                    <span className={styles.pickerOptionMeta}>{option.meta}</span>
                    <span className={styles.pickerCheck}>
                      {option.value === value && <Icon name="check" size={14} />}
                    </span>
                  </button>
                ))}
              </div>
            ) : (
              <p role="status" className={styles.pickerNone}>
                Nothing matches.
              </p>
            )}
          </div>
        </Popover>
      )}
    </>
  );
}
