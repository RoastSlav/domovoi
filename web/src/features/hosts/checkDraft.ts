import type { Overrides } from "../../api/hooks/settings";
import type { HostCheck, Issue } from "../../api/types";

/** Whether a check runs: as the agent says, or the hub forces it on or off. */
export type Runs = "" | "on" | "off";

/** What the edit form holds for one check, as the person typed it. */
export interface Draft {
  interval: string;
  runs: Runs;
  thresholds: string;
}

/** What is wrong with each part of a draft; `check` is about the check as a whole. */
export interface Problems {
  check?: string;
  interval?: string;
  runs?: string;
  thresholds?: string;
}

/** The form's values for the overrides a check has saved. */
export function draftOf(check: HostCheck): Draft {
  const o = check.overrides;
  return {
    runs: o?.enabled == null ? "" : o.enabled ? "on" : "off",
    interval: o?.interval_seconds == null ? "" : String(o.interval_seconds),
    thresholds:
      o?.thresholds && Object.keys(o.thresholds).length > 0 ? JSON.stringify(o.thresholds) : "",
  };
}

/** Whether two drafts say the same thing. */
export function sameDraft(a: Draft, b: Draft): boolean {
  return (
    a.runs === b.runs && a.interval.trim() === b.interval.trim() && a.thresholds === b.thresholds
  );
}

/** What a valid draft overrides: `null` fields keep the agent's value. */
export type Override = NonNullable<Overrides[string]>;

/** The override a draft describes, `null` when it overrides nothing, or what is wrong with it. */
export function parseDraft(draft: Draft): { override: Override | null } | { problems: Problems } {
  const problems: Problems = {};
  let thresholds: Record<string, unknown> = {};
  if (draft.thresholds.trim() !== "") {
    try {
      const parsed: unknown = JSON.parse(draft.thresholds);
      if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
        problems.thresholds = 'Thresholds must be a JSON object, like {"warn_percent": 70}.';
      } else {
        thresholds = parsed as Record<string, unknown>;
      }
    } catch {
      problems.thresholds = "Thresholds are not valid JSON. Check the braces and quotes.";
    }
  }
  const interval = draft.interval.trim() === "" ? null : Number(draft.interval);
  if (interval !== null && (!Number.isInteger(interval) || interval < 1)) {
    problems.interval = "The interval is a whole number of seconds, at least 1.";
  }
  if (Object.keys(problems).length > 0) {
    return { problems };
  }
  if (draft.runs === "" && interval === null && Object.keys(thresholds).length === 0) {
    return { override: null };
  }
  return {
    override: {
      enabled: draft.runs === "" ? null : draft.runs === "on",
      interval_seconds: interval,
      thresholds,
    },
  };
}

/** The overrides to send for these drafts; a draft that overrides nothing is left out. */
export function overridesOf(drafts: Record<string, Draft>): Overrides {
  const out: Overrides = {};
  for (const [key, draft] of Object.entries(drafts)) {
    const parsed = parseDraft(draft);
    if ("override" in parsed && parsed.override) {
      out[key] = parsed.override;
    }
  }
  return out;
}

/** How a check stands with `draft` applied: what runs, how often, with which thresholds, and from where. */
export interface Effective {
  enabled: boolean | undefined;
  interval: number | undefined;
  source: "agent" | "hub";
  thresholds: Record<string, unknown>;
}

/** The values a check ends up with: the draft's overrides over what the agent reported. */
export function effectiveOf(check: HostCheck, draft: Draft): Effective {
  const parsed = parseDraft(draft);
  const override = "override" in parsed ? parsed.override : null;
  const reported = check.reported;
  return {
    enabled: override?.enabled ?? reported?.enabled ?? undefined,
    interval: override?.interval_seconds ?? reported?.interval_seconds ?? undefined,
    thresholds: { ...(reported?.thresholds ?? {}), ...(override?.thresholds ?? {}) },
    source: override ? "hub" : "agent",
  };
}

/** The hub's issues for one check, by the part of the form they are about. */
export function problemsFor(key: string, issues: readonly Issue[]): Problems {
  const out: Problems = {};
  for (const issue of issues) {
    if (issue.path === key) {
      out.check = issue.message;
    } else if (issue.path.startsWith(`${key}.`)) {
      const part = issue.path.slice(key.length + 1).split(".")[0];
      if (part === "enabled") {
        out.runs = issue.message;
      } else if (part === "interval_seconds") {
        out.interval = issue.message;
      } else {
        out.thresholds = issue.message;
      }
    }
  }
  return out;
}
