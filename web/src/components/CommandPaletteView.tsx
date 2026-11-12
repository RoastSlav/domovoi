import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { Icon } from "../design/icons";
import type { CommandPaletteProps } from "./CommandPalette";
import { Highlight } from "./Highlight";
import { Kbd } from "./Kbd";
import { filterGroups, type PaletteItem } from "./palette";
import styles from "./palette.module.css";

/**
 * The palette itself, loaded on demand by `CommandPalette`. It is a native modal `<dialog>`
 * (focus stays inside, the page is held still behind it) holding an ARIA combobox: the
 * search input owns the focus and points at the active option with `aria-activedescendant`.
 */
export default function CommandPaletteView({
  open,
  onClose,
  groups,
  placeholder = "Search pages, hosts and actions…",
}: CommandPaletteProps) {
  return open ? <Palette onClose={onClose} groups={groups} placeholder={placeholder} /> : null;
}

function Palette({
  onClose,
  groups,
  placeholder,
}: Omit<CommandPaletteProps, "open"> & { placeholder: string }) {
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const dialog = useRef<HTMLDialogElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const base = useId();
  const listId = `${base}-list`;

  const shown = useMemo(() => filterGroups(groups, query), [groups, query]);
  const flat = useMemo(() => shown.flatMap((group) => group.items), [shown]);
  const current = Math.min(active, flat.length - 1);
  const activeItem = flat[current];
  const activeId = activeItem ? `${base}-opt-${activeItem.id}` : undefined;

  useEffect(() => {
    const node = dialog.current;
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    if (node && !node.open) {
      node.showModal();
    }
    input.current?.focus();
    return () => {
      if (node?.open) {
        node.close();
      }
      opener?.focus();
    };
  }, []);

  useEffect(() => {
    // jsdom has no layout, so the call may be missing.
    document.getElementById(activeId ?? "")?.scrollIntoView?.({ block: "nearest" });
  }, [activeId]);

  const choose = (item: PaletteItem): void => {
    onClose();
    item.onSelect();
  };

  const onKeyDown = (event: KeyboardEvent): void => {
    const last = flat.length - 1;
    const move = (to: number): void => {
      event.preventDefault();
      if (last >= 0) {
        setActive(to);
      }
    };
    switch (event.key) {
      case "ArrowDown":
        move(current >= last ? 0 : current + 1);
        break;
      case "ArrowUp":
        move(current <= 0 ? last : current - 1);
        break;
      case "Home":
        move(0);
        break;
      case "End":
        move(last);
        break;
      case "Enter":
        event.preventDefault();
        if (activeItem && !event.nativeEvent.isComposing) {
          choose(activeItem);
        }
        break;
      case "Escape":
        event.preventDefault();
        event.stopPropagation();
        onClose();
        break;
      case "Tab":
        // The input is the only stop: the options are reached with the arrow keys.
        event.preventDefault();
        break;
      default:
    }
  };

  return (
    <dialog
      ref={dialog}
      className={styles.palette}
      aria-label="Command palette"
      aria-modal="true"
      onKeyDown={onKeyDown}
      onCancel={(event) => {
        event.preventDefault();
        onClose();
      }}
      onClick={(event) => {
        // A click on the backdrop lands on the dialog itself.
        if (event.target === dialog.current) {
          onClose();
        }
      }}
    >
      <div className={styles.search}>
        <Icon name="search" />
        <input
          ref={input}
          type="text"
          role="combobox"
          className={styles.input}
          aria-label="Search"
          aria-autocomplete="list"
          aria-expanded={flat.length > 0}
          aria-controls={flat.length > 0 ? listId : undefined}
          aria-activedescendant={activeId}
          autoComplete="off"
          spellCheck={false}
          placeholder={placeholder}
          value={query}
          onChange={(event) => {
            setQuery(event.target.value);
            setActive(0);
          }}
        />
      </div>
      <div className={styles.results}>
        {flat.length > 0 ? (
          <div role="listbox" id={listId} aria-label="Results">
            {shown.map((group, index) => (
              <div key={group.title} role="group" aria-labelledby={`${base}-g-${index}`}>
                <div className={styles.groupTitle} id={`${base}-g-${index}`}>
                  {group.title}
                </div>
                {group.items.map((item) => (
                  <div
                    key={item.id}
                    id={`${base}-opt-${item.id}`}
                    role="option"
                    aria-selected={item === activeItem}
                    className={styles.option}
                    onClick={() => choose(item)}
                    onMouseMove={() => setActive(flat.indexOf(item))}
                  >
                    {item.icon && <Icon name={item.icon} />}
                    <span className={styles.label}>
                      <Highlight text={item.label} query={query} />
                    </span>
                    {item.hint && (
                      <>
                        {" "}
                        <span className={styles.hint}>{item.hint}</span>
                      </>
                    )}
                  </div>
                ))}
              </div>
            ))}
          </div>
        ) : (
          <p className={styles.none} role="status">
            Nothing matches
          </p>
        )}
      </div>
      <div className={styles.footer} aria-hidden="true">
        <span>
          <Kbd>↑</Kbd> <Kbd>↓</Kbd> to move
        </span>
        <span>
          <Kbd>Enter</Kbd> to open
        </span>
        <span>
          <Kbd>Esc</Kbd> to close
        </span>
      </div>
    </dialog>
  );
}
