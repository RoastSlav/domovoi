import { useState } from "react";
import { Icon } from "../design/icons";
import type { TimelineMeta } from "../design/timeline";
import { Button } from "./Button";
import { RelativeTime } from "./RelativeTime";
import styles from "./data.module.css";

/** One thing that happened: how it reads (from `timelineMeta`) and when. */
export interface TimelineItem {
  at: string;
  meta: TimelineMeta;
}

/** How many entries a long history shows before "Show all". */
export const TIMELINE_PREVIEW = 8;

/**
 * What happened to one thing, oldest first: a toned dot with an icon, a short phrase, the
 * detail under it and the time (exact on hover). A long history shows its newest entries
 * and a "Show all N" button.
 *
 * Use it for the history of a single object. Get each entry's `meta` from `timelineMeta`
 * in `src/design/timeline.ts`; do not write phrases in the page.
 */
export function Timeline({ items, label }: { items: readonly TimelineItem[]; label: string }) {
  const [all, setAll] = useState(false);
  const hidden = all ? 0 : Math.max(0, items.length - TIMELINE_PREVIEW);
  const shown = items.slice(hidden);
  return (
    <div className={styles.timelineWrap}>
      {hidden > 0 && (
        <Button size="sm" variant="ghost" onClick={() => setAll(true)}>
          Show all {items.length}
        </Button>
      )}
      <ol className={styles.timeline} aria-label={label}>
        {shown.map((item, index) => (
          <li key={`${item.at}-${hidden + index}`} className={styles.timelineItem}>
            <span className={styles.timelineDot} data-tone={item.meta.tone}>
              <Icon name={item.meta.icon} size={14} />
            </span>
            <div className={styles.timelineText}>
              <p className={styles.timelinePhrase}>
                <span>{item.meta.phrase}</span>
                <span className={styles.timelineTime}>
                  <RelativeTime at={item.at} />
                </span>
              </p>
              {item.meta.detail && <p className={styles.timelineDetail}>{item.meta.detail}</p>}
            </div>
          </li>
        ))}
      </ol>
    </div>
  );
}
