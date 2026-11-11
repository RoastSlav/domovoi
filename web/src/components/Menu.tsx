import { useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { Icon, type IconName } from "../design/icons";
import { Button } from "./Button";
import { IconButton } from "./IconButton";
import { Popover } from "./Popover";
import styles from "./overlay.module.css";

/** One entry in a menu. */
export interface MenuItem {
  /** Set (true or false) for one of several exclusive choices; the chosen one shows a check. */
  checked?: boolean;
  /** Why it cannot be chosen now; shown under the label. */
  disabledReason?: string | undefined;
  icon?: IconName;
  label: string;
  onSelect: () => void;
  /** `danger` for a destructive entry (which still opens a `ConfirmDialog`). */
  tone?: "danger";
}

/**
 * Secondary actions behind a "More" icon button. Arrow keys, Home and End move between
 * entries; Enter chooses; Escape closes and returns focus to the button. `icon` changes the
 * button's icon, `header` is a line above the entries (who is signed in), and an entry with
 * `checked` is one of several exclusive choices. `text` makes the trigger a button with that
 * word and a chevron, for a menu of choices that complete it ("Mute", then "For 1 hour").
 *
 * Use it when a row or header has more than two or three actions. Keep the main action as a
 * visible `Button`; never hide the only way to do something important in a menu.
 */
export function Menu({
  label = "More actions",
  items,
  icon = "more",
  header,
  text,
  hint,
  loading,
}: {
  header?: ReactNode;
  /** With `text`: what choosing from the menu does, as the button's tooltip. */
  hint?: string;
  /** With `text`: the chosen entry is running. */
  loading?: boolean;
  /** Shows the trigger as a button with this word; `label` stays its accessible name. */
  text?: string;
  icon?: IconName;
  items: MenuItem[];
  label?: string;
}) {
  const [open, setOpen] = useState(false);
  const button = useRef<HTMLButtonElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const id = useId();

  const entries = (): HTMLElement[] => [
    ...(list.current?.querySelectorAll<HTMLElement>('[role^="menuitem"]') ?? []),
  ];
  useEffect(() => {
    if (open) {
      entries()[0]?.focus();
    }
  }, [open]);

  const onKeyDown = (event: KeyboardEvent): void => {
    const all = entries();
    const at = all.indexOf(document.activeElement as HTMLElement);
    const to =
      event.key === "ArrowDown"
        ? (at + 1) % all.length
        : event.key === "ArrowUp"
          ? (at - 1 + all.length) % all.length
          : event.key === "Home"
            ? 0
            : event.key === "End"
              ? all.length - 1
              : -1;
    if (to >= 0) {
      event.preventDefault();
      all[to]?.focus();
    }
  };

  return (
    <>
      {text ? (
        <Button
          ref={button}
          aria-label={label}
          aria-haspopup="menu"
          aria-expanded={open}
          aria-controls={open ? id : undefined}
          hint={hint}
          loading={loading}
          onClick={() => setOpen((now) => !now)}
        >
          {text}
          <Icon name="chevron-down" size={14} />
        </Button>
      ) : (
        <IconButton
          ref={button}
          icon={icon}
          label={label}
          aria-haspopup="menu"
          aria-expanded={open}
          aria-controls={open ? id : undefined}
          onClick={() => setOpen((now) => !now)}
        />
      )}
      {open && (
        <Popover anchor={button} id={id} label={label} role="menu" onClose={() => setOpen(false)}>
          <div ref={list} onKeyDown={onKeyDown}>
            {header && <div className={styles.menuHeader}>{header}</div>}
            {items.map((item) => (
              <button
                key={item.label}
                type="button"
                role={item.checked === undefined ? "menuitem" : "menuitemradio"}
                aria-checked={item.checked}
                tabIndex={-1}
                className={styles.menuItem}
                data-tone={item.tone}
                aria-disabled={item.disabledReason ? true : undefined}
                onClick={() => {
                  if (item.disabledReason) {
                    return;
                  }
                  setOpen(false);
                  button.current?.focus();
                  item.onSelect();
                }}
              >
                {item.icon && <Icon name={item.icon} />}
                <span className={styles.menuLabel}>
                  {item.label}
                  {item.disabledReason && <small> ({item.disabledReason})</small>}
                </span>
                {item.checked && <Icon name="check" size={14} />}
              </button>
            ))}
          </div>
        </Popover>
      )}
    </>
  );
}
