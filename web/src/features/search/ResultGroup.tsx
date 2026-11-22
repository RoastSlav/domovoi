import { useState, type ReactNode } from "react";
import { Button, LoadError, Section, Skeleton, Visible } from "../../components";
import styles from "./search.module.css";
import type { GroupState } from "./useSearchGroups";

/** How many rows a group shows before "Show all". */
export const VISIBLE_ROWS = 5;

/** Marks one result group; each id is a literal so the visibility table can be checked from the sources. */
function GroupVisible({ id, children }: { id: string; children: ReactNode }) {
  switch (id) {
    case "incidents":
      return (
        <Visible category="P1" id="search.incidents">
          {children}
        </Visible>
      );
    case "operations":
      return (
        <Visible category="P1" id="search.operations">
          {children}
        </Visible>
      );
    case "past":
      return (
        <Visible category="P1" id="search.past">
          {children}
        </Visible>
      );
    case "notes":
      return (
        <Visible category="P1" id="search.notes">
          {children}
        </Visible>
      );
    case "facts":
      return (
        <Visible category="P1" id="search.facts">
          {children}
        </Visible>
      );
    default:
      return (
        <Visible category="P1" id="search.hosts">
          {children}
        </Visible>
      );
  }
}

/** One result of a group: a stable key and what to draw. */
export interface ResultRow {
  key: string;
  node: ReactNode;
}

/**
 * One source's results as a titled section: skeleton rows while it loads, its own error with
 * Retry if it failed, a plain "no match" line, or the first five rows with "Show all" for the
 * rest of what was already loaded. A group in the idle state draws nothing.
 */
export function ResultGroup({
  group,
  state,
  rows,
  actions,
  lead,
}: {
  actions?: ReactNode;
  group: { id: string; label: string };
  /** Drawn under the heading in every state, before the rows. */
  lead?: ReactNode;
  rows: ResultRow[];
  state: GroupState<unknown>;
}) {
  const [all, setAll] = useState(false);
  if (state.status === "idle") {
    return null;
  }
  const lower = group.label.toLowerCase();
  const shown = all ? rows : rows.slice(0, VISIBLE_ROWS);
  return (
    <GroupVisible id={group.id}>
      <Section title={group.label} actions={actions}>
        {lead}
        {state.status === "pending" && <Skeleton shape="rows" count={2} />}
        {state.status === "error" && (
          <LoadError context={`search ${lower}`} error={state.error} onRetry={state.retry} />
        )}
        {state.status === "ready" && rows.length === 0 && (
          <p className={styles.muted}>No {lower} match.</p>
        )}
        {state.status === "ready" && rows.length > 0 && (
          <ul className={styles.rows}>
            {shown.map((row) => (
              <li key={row.key} className={styles.row}>
                {row.node}
              </li>
            ))}
          </ul>
        )}
        {state.status === "ready" && !all && state.total > VISIBLE_ROWS && (
          <div className={styles.more}>
            <Button variant="ghost" size="sm" onClick={() => setAll(true)}>
              Show all {state.total}
            </Button>
          </div>
        )}
        {state.status === "ready" && state.note && <p className={styles.muted}>{state.note}</p>}
      </Section>
    </GroupVisible>
  );
}
