import { useId, useState, type KeyboardEvent, type ReactNode } from "react";
import styles from "./ui.module.css";

/** One tab: its label and what it shows. */
export interface Tab {
  label: string;
  content: ReactNode;
}

/** Tabs with arrow-key navigation; only the selected panel is rendered. */
export function Tabs({ tabs, label }: { tabs: Tab[]; label: string }) {
  const [selected, setSelected] = useState(0);
  const base = useId();
  const onKeyDown = (event: KeyboardEvent) => {
    const step = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
    if (step !== 0) {
      event.preventDefault();
      setSelected((current) => (current + step + tabs.length) % tabs.length);
    }
  };
  const current = tabs[selected];
  return (
    <>
      <div role="tablist" aria-label={label} className={styles.tabs} onKeyDown={onKeyDown}>
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
          </button>
        ))}
      </div>
      <div role="tabpanel" id={`${base}-panel`} aria-labelledby={`${base}-tab-${selected}`}>
        {current?.content}
      </div>
    </>
  );
}
