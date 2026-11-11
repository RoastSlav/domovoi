import { useLayoutEffect, useMemo, useRef, useState } from "react";
import type { LogLine } from "../../api/types";
import {
  Button,
  CopyButton,
  EmptyState,
  Field,
  QueryBoundary,
  Skeleton,
  Switch,
  Text,
  TextInput,
  useToast,
} from "../../components";
import { describeError } from "../../design/messages";
import { LogRows } from "./LogRows";
import { PAGE_LINES, formatLine, lineMatches } from "./logLines";
import styles from "./logs.module.css";

export { DEFAULT_LEVEL, DEFAULT_LINES } from "./logLines";

/** How close to the bottom, in pixels, still counts as reading the newest line. */
const AT_BOTTOM_PX = 24;

/** The part of a TanStack query the viewer reads. */
export interface LogsQuery {
  data: LogLine[] | undefined;
  error: unknown;
  isPending: boolean;
  refetch: () => unknown;
}

/** The log area's shape while the first lines load, so nothing shifts when they arrive. */
function LogSkeleton() {
  return (
    <div className={styles.frame}>
      <div className={styles.area}>
        <Skeleton shape="line" count={12} />
      </div>
    </div>
  );
}

/** The lines themselves: newest at the bottom, kept there unless the reader scrolled up. */
function LogArea({
  lines,
  query,
  label,
  wrap,
  level,
  refreshMs,
}: {
  label: string;
  level: string;
  lines: LogLine[];
  query: string;
  refreshMs: number | null;
  wrap: boolean;
}) {
  const toast = useToast();
  const areaRef = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);
  const [away, setAway] = useState(false);
  const [cap, setCap] = useState(PAGE_LINES);
  const visible = useMemo(() => lines.slice(-cap), [lines, cap]);
  const hidden = lines.length - visible.length;

  const toBottom = () => {
    const area = areaRef.current;
    if (area) {
      area.scrollTop = area.scrollHeight;
    }
  };
  useLayoutEffect(() => {
    if (pinned.current) {
      toBottom();
    }
  }, [visible]);

  const onScroll = () => {
    const area = areaRef.current;
    if (area) {
      const atBottom = area.scrollHeight - area.scrollTop - area.clientHeight < AT_BOTTOM_PX;
      pinned.current = atBottom;
      setAway(!atBottom);
    }
  };

  if (lines.length === 0) {
    const searching = query.trim() !== "";
    return (
      <EmptyState title="No log lines">
        <p>
          {searching
            ? "No line in this log matches the search. Clear it to see every line."
            : `Nothing was logged at ${level} or above. Lower the level to see more.`}
        </p>
      </EmptyState>
    );
  }

  const total = lines.length;
  const shown = visible.length === total ? `${total} lines` : `${visible.length} of ${total} lines`;
  return (
    <>
      <div className={styles.frame}>
        <div
          ref={areaRef}
          role="log"
          aria-live="off"
          aria-label={label}
          tabIndex={0}
          className={styles.area}
          data-wrap={wrap || undefined}
          onScroll={onScroll}
        >
          {hidden > 0 && (
            <div className={styles.older}>
              <Button size="sm" icon="chevron-up" onClick={() => setCap(cap + PAGE_LINES)}>
                Show older
              </Button>
              <Text muted size="sm">
                {`${hidden} older lines are not drawn`}
              </Text>
            </div>
          )}
          <LogRows lines={visible} query={query} />
        </div>
        {away && (
          <div className={styles.jump}>
            <Button
              size="sm"
              variant="primary"
              icon="chevron-down"
              onClick={() => {
                pinned.current = true;
                toBottom();
                setAway(false);
              }}
            >
              Jump to latest
            </Button>
          </div>
        )}
      </div>
      <div className={styles.footer}>
        <Text muted size="sm">
          {shown}
        </Text>
        {refreshMs !== null && (
          <Text muted size="sm">
            {`Auto-refresh every ${refreshMs / 1000} s`}
          </Text>
        )}
        <CopyButton
          label="Copy visible lines"
          text={visible.map(formatLine).join("\n")}
          onUnavailable={() =>
            toast.error(describeError(new Error("clipboard unavailable"), "copy the log lines"))
          }
        />
      </div>
    </>
  );
}

/**
 * Log lines oldest first, newest at the bottom, in monospace with only the level word toned.
 * A search box filters and highlights, a wrap switch folds long lines, and only the newest
 * 500 lines are drawn until "Show older" asks for more, so a long log does not slow the page.
 * It says what it is doing while loading, why it is empty and why it could not load.
 *
 * `refreshMs` is how often the lines are asked for again, or `null` when they are not (the
 * auto-refresh line is then hidden). With `onLiveChange` a "Live" switch, on while `refreshMs`
 * is set, lets the reader turn the asking on and off; the page owns what that means.
 * Shared by the Logs page and the Logs tab of the host page.
 */
export function LogViewer({
  query,
  level,
  label,
  refreshMs,
  onLiveChange,
}: {
  label: string;
  level: string;
  onLiveChange?: (live: boolean) => void;
  query: LogsQuery;
  refreshMs: number | null;
}) {
  const [search, setSearch] = useState("");
  const [wrap, setWrap] = useState(false);
  const all = query.data;
  const matching = useMemo(
    () => (all ?? []).filter((line) => lineMatches(line, search)),
    [all, search],
  );
  // A new search starts from the newest lines again.
  const areaKey = search.trim();

  return (
    <div className={styles.viewer}>
      <div className={styles.toolbar}>
        <div className={styles.search}>
          <Field label="Search lines">
            <TextInput
              type="search"
              placeholder="Text, level or source"
              value={search}
              onChange={(event) => setSearch(event.target.value)}
            />
          </Field>
        </div>
        <div className={styles.switches}>
          {onLiveChange && (
            <Switch label="Live" checked={refreshMs !== null} onChange={onLiveChange} />
          )}
          <Switch label="Wrap lines" checked={wrap} onChange={setWrap} />
          <Button icon="refresh" onClick={() => void query.refetch()}>
            Refresh
          </Button>
        </div>
      </div>
      <QueryBoundary
        query={{ ...query, data: all === undefined ? undefined : matching }}
        context="load logs"
        loading={<LogSkeleton />}
      >
        {(lines) => (
          <LogArea
            key={areaKey}
            lines={lines}
            query={search}
            label={label}
            wrap={wrap}
            level={level}
            refreshMs={refreshMs}
          />
        )}
      </QueryBoundary>
    </div>
  );
}
