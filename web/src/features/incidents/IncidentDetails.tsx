import { useState } from "react";
import { Button, Markdown, Section } from "../../components";
import styles from "./incidents.module.css";

/** About this many lines show before "Show full details". */
const PREVIEW_LINES = 6;

/** Whether `text` is long enough to fold. */
export function isLong(text: string): boolean {
  return text.split("\n").length > PREVIEW_LINES || text.length > 600;
}

/**
 * The report's long-form details as Markdown: the first six lines or so, the rest behind
 * "Show full details", so a long write-up with headings and code never pushes the chat and
 * the history off the screen.
 */
export function IncidentDetails({ text }: { text: string }) {
  const [full, setFull] = useState(false);
  const long = isLong(text);
  return (
    <Section title="Details">
      <div className={styles.details} data-folded={long && !full ? "" : undefined}>
        <Markdown text={text} />
      </div>
      {long && (
        <Button
          size="sm"
          variant="ghost"
          icon={full ? "chevron-up" : "chevron-down"}
          aria-expanded={full}
          onClick={() => setFull((now) => !now)}
        >
          {full ? "Show less" : "Show full details"}
        </Button>
      )}
    </Section>
  );
}
