import { useId, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import type { Tone } from "../design/tone";
import { Count } from "./Count";
import styles from "./data.module.css";

/** One tab: its label, what it shows, and an optional count beside the label. */
export interface Tab {
  content: ReactNode;
  count?: { label: string; tone?: Tone; value: number };
  label: string;
}

/**
 * Tabs: the arrow keys, Home and End select the neighbouring tab and move focus to it; only
 * the selected panel is rendered.
 *
 * Use it for a few views of the same object (a host's checks, logs, profile). Different
 * places in the app are navigation, not tabs.
 */
export function Tabs({ tabs, label }: { label: string; tabs: Tab[] }) {
  const [selected, setSelected] = useState(0);
  const base = useId();
  const list = useRef<HTMLDivElement>(null);

  const onKeyDown = (event: KeyboardEvent) => {
    const last = tabs.length - 1;
    const to =
      event.key === "ArrowRight"
        ? (selected + 1) % tabs.length
        : event.key === "ArrowLeft"
          ? (selected + last) % tabs.length
          : event.key === "Home"
            ? 0
            : event.key === "End"
              ? last
              : -1;
    if (to < 0) {
      return;
    }
    event.preventDefault();
    setSelected(to);
    list.current?.querySelectorAll<HTMLElement>('[role="tab"]')[to]?.focus();
  };

  const current = tabs[selected];
  return (
    <>
      <div
        ref={list}
        role="tablist"
        aria-label={label}
        className={styles.tabs}
        onKeyDown={onKeyDown}
      >
        {tabs.map((tab, index) => (
          <button
            key={tab.label}
            type="button"
            role="tab"
            id={`${base}-tab-${index}`}
            aria-selected={index === selected}
            aria-controls={`${base}-panel`}
            tabIndex={index === selected ? 0 : -1}
            className={styles.tab}
            onClick={() => setSelected(index)}
          >
            {tab.label}
            {tab.count && tab.count.value > 0 && (
              <Count
                tone={tab.count.tone ?? "neutral"}
                value={tab.count.value}
                label={tab.count.label}
              />
            )}
          </button>
        ))}
      </div>
      <div role="tabpanel" id={`${base}-panel`} aria-labelledby={`${base}-tab-${selected}`}>
        {current?.content}
      </div>
    </>
  );
}
