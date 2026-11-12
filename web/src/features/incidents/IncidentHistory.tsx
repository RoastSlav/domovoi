import { useState } from "react";
import type { IncidentDetail, TimelineEntry } from "../../api/types";
import { Button, InfoTip, RelativeTime, Section, Text, TIMELINE_PREVIEW } from "../../components";
import { Icon } from "../../design/icons";
import { timelineMeta } from "../../design/timeline";
import styles from "./incidents.module.css";

type ToolUsage = IncidentDetail["tools_used"][number];

/** "14 tools · 43 calls · 4 failed · 6 not run"; the last two only when there are some. */
export function toolsSummary(tools: readonly ToolUsage[]): string {
  const sum = (pick: (tool: ToolUsage) => number) => tools.reduce((n, tool) => n + pick(tool), 0);
  const failed = sum((tool) => tool.failures);
  const refused = sum((tool) => tool.refused);
  return [
    tools.length === 1 ? "1 tool" : `${tools.length} tools`,
    `${sum((tool) => tool.calls)} calls`,
    failed > 0 ? `${failed} failed` : "",
    refused > 0 ? `${refused} not run` : "",
  ]
    .filter(Boolean)
    .join(" · ");
}

/**
 * What triage and the follow-up chat looked at, as one summary line that opens a dense list:
 * a name in mono and its calls, with failures and calls that were not run only when there are
 * some. The sentence about read tools sits in the heading's info button.
 */
export function ToolsUsed({ tools }: { tools: ToolUsage[] }) {
  const [open, setOpen] = useState(false);
  return (
    <Section
      title="Skills and tools used"
      actions={
        <InfoTip title="skills and tools used">
          Read tools and read skills only look; they never change the host.
        </InfoTip>
      }
    >
      {tools.length === 0 ? (
        <Text block muted size="sm">
          Nothing used yet.
        </Text>
      ) : (
        <>
          <Button
            size="sm"
            variant="ghost"
            icon={open ? "chevron-down" : "chevron-right"}
            aria-expanded={open}
            onClick={() => setOpen((now) => !now)}
          >
            {toolsSummary(tools)}
          </Button>
          {open && (
            <ul className={styles.toolList}>
              {tools.map((tool) => (
                <li key={`${tool.kind}-${tool.name}`} className={styles.toolRow}>
                  <span className={styles.toolName}>{tool.name}</span>
                  <span className={styles.toolKind}>{tool.kind}</span>
                  <span className={styles.toolFigures}>
                    {tool.calls} {tool.calls === 1 ? "call" : "calls"}
                    {tool.failures > 0 && (
                      <span className={styles.toolFailed}>{tool.failures} failed</span>
                    )}
                    {tool.refused > 0 && (
                      <span className={styles.toolRefused}>{tool.refused} not run</span>
                    )}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </>
      )}
    </Section>
  );
}

/** Everything that happened to the incident, in words, newest first, one line each. */
export function IncidentTimeline({ entries }: { entries: TimelineEntry[] }) {
  const [all, setAll] = useState(false);
  // The hub lists oldest first; what just happened is what the person came for.
  const newest = [...entries].reverse();
  const shown = all ? newest : newest.slice(0, TIMELINE_PREVIEW);
  return (
    <Section title="Timeline">
      {entries.length === 0 ? (
        <Text block muted size="sm">
          Nothing yet.
        </Text>
      ) : (
        <>
          <ol className={styles.timeline} aria-label="Timeline">
            {shown.map((entry, index) => {
              const meta = timelineMeta(entry);
              return (
                <li key={`${entry.at}-${index}`} className={styles.timelineRow}>
                  <span className={styles.timelineIcon} data-tone={meta.tone}>
                    <Icon name={meta.icon} size={14} />
                  </span>
                  <span className={styles.timelineText}>
                    <span className={styles.timelinePhrase}>{meta.phrase}</span>
                    {meta.detail && <span className={styles.timelineDetail}> {meta.detail}</span>}
                  </span>
                  <span className={styles.timelineWhen}>
                    <RelativeTime at={entry.at} />
                  </span>
                </li>
              );
            })}
          </ol>
          {entries.length > TIMELINE_PREVIEW && (
            <Button size="sm" variant="ghost" onClick={() => setAll((now) => !now)}>
              {all ? "Show fewer" : `Show all ${entries.length}`}
            </Button>
          )}
        </>
      )}
    </Section>
  );
}
