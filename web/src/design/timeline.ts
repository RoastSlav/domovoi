import type { IconName } from "./icons";
import { actionStatusMeta, incidentStatusMeta, verdictMeta, type Tone } from "./tone";

/**
 * The single place where a timeline entry becomes words. The hub records each entry with a
 * dotted kind (`triage.report`) and a one-line summary; people read a phrase ("Triage
 * reported") with the summary as its detail. A kind this table does not know is shown as
 * it is, so a newer hub never breaks the page.
 */

/** How one timeline entry is shown. */
export interface TimelineMeta {
  /** The rest of the story, in the hub's words; absent when the phrase says it all. */
  detail?: string;
  icon: IconName;
  /** What happened, in a few words. */
  phrase: string;
  tone: Tone;
}

/** What the page needs of a timeline entry. */
export interface TimelineInput {
  kind: string;
  summary: string;
}

interface Entry {
  /** `false` hides the summary: it repeats the phrase, or is shown elsewhere on the page. */
  detail?: false;
  icon: IconName;
  phrase: string;
  tone: Tone;
}

const KINDS: Record<string, Entry> = {
  "incident.created": { phrase: "Opened", tone: "pending", icon: "alert-circle" },
  // Hubs before 0.1 called it this.
  "incident.opened": { phrase: "Opened", tone: "pending", icon: "alert-circle" },
  "incident.grouped": { phrase: "A related check joined", tone: "neutral", icon: "plus" },
  "human.dismissed": { phrase: "Dismissed as a known condition", tone: "neutral", icon: "check" },
  "triage.report": { phrase: "Triage reported", tone: "info", icon: "note" },
  "triage.cached": { phrase: "Triage reused an earlier report", tone: "info", icon: "note" },
  "triage.skipped": { phrase: "Triage was skipped", tone: "warning", icon: "pause" },
  "triage.failed": { phrase: "Triage failed", tone: "danger", icon: "x-circle" },
  "triage.tools": { phrase: "Tools were used", tone: "neutral", icon: "tool" },
  // The question itself is in the chat on the same page.
  "triage.followup": {
    phrase: "You asked a follow-up",
    tone: "neutral",
    icon: "chat",
    detail: false,
  },
  "followup.cleared": {
    phrase: "The conversation was cleared",
    tone: "neutral",
    icon: "chat",
    detail: false,
  },
  "ack_rule.created": { phrase: "Known condition added", tone: "neutral", icon: "rules" },
  "ack_rule.deleted": { phrase: "Known condition removed", tone: "neutral", icon: "rules" },
  "action.blocked": { phrase: "Action stopped by the loop guard", tone: "warning", icon: "ban" },
};

/** What a change to each status reads as; the tone and icon are the status's own. */
const STATUS_PHRASES: Record<string, string> = {
  pending: "Waiting for confirmation",
  open: "Confirmed",
  acked: "Acknowledged",
  muted: "Muted",
  resolved: "Resolved",
};

const ACTION_PREFIX = "action.";

/** "open → acked" as the status it changed to, or nothing when it does not read that way. */
function changedTo(summary: string): string | undefined {
  const to = summary.split("→").at(-1)?.trim();
  return to && summary.includes("→") ? to : undefined;
}

/** How timeline `entry` is shown: a phrase, a tone, an icon and the hub's line as detail. */
export function timelineMeta(entry: TimelineInput): TimelineMeta {
  const summary = entry.summary.trim();
  const withDetail = (meta: Omit<TimelineMeta, "detail">, show = true): TimelineMeta =>
    show && summary ? { ...meta, detail: summary } : meta;

  if (entry.kind === "status.changed") {
    const to = changedTo(summary);
    if (to) {
      const status = incidentStatusMeta(to);
      return { phrase: STATUS_PHRASES[to] ?? status.label, tone: status.tone, icon: status.icon };
    }
    return withDetail({ phrase: "Status changed", tone: "neutral", icon: "dot" });
  }
  if (entry.kind === "verdict") {
    // The summary is the verdict as the chat card prints it: "ACTION NEEDED".
    const verdict = verdictMeta(summary.replaceAll(" ", "_"));
    return { phrase: `Triage said ${verdict.label}`, tone: verdict.tone, icon: verdict.icon };
  }
  if (Object.hasOwn(KINDS, entry.kind)) {
    const known = KINDS[entry.kind] as Entry;
    const { detail, ...meta } = known;
    return withDetail(meta, detail !== false);
  }
  if (entry.kind.startsWith(ACTION_PREFIX)) {
    const status = actionStatusMeta(entry.kind.slice(ACTION_PREFIX.length));
    return withDetail({ phrase: `Action: ${status.label}`, tone: status.tone, icon: status.icon });
  }
  return withDetail({ phrase: entry.kind, tone: "neutral", icon: "dot" });
}

/** Every kind the table knows, for tests and the design catalogue. */
export const TIMELINE_KINDS = [...Object.keys(KINDS), "status.changed", "verdict"];
