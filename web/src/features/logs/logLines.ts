import type { LogLine } from "../../api/types";

/** How many of the newest lines are drawn, and how many more "Show older" reveals. */
export const PAGE_LINES = 500;

/** The four levels a line is toned by; anything else reads as `info`. */
export type LevelKind = "error" | "warn" | "info" | "debug";

/** The level and line count asked for when a reader has not picked. */
export const DEFAULT_LEVEL = "info";
export const DEFAULT_LINES = 200;

/**
 * The level a line is toned by, from the level text the hub or agent sent. Common spellings
 * fold into the four kinds ("warning" is `warn`, "fatal" is `error`, "trace" is `debug`);
 * an unknown level stays plain `info`, so a newer sender never breaks the page.
 */
export function levelKind(level: string): LevelKind {
  const text = level.trim().toLowerCase();
  if (["error", "err", "fatal", "critical", "crit"].includes(text)) {
    return "error";
  }
  if (["warn", "warning"].includes(text)) {
    return "warn";
  }
  if (["debug", "trace"].includes(text)) {
    return "debug";
  }
  return "info";
}

/** The time of day a line was logged, as `HH:MM:SS`, in the browser's own zone. */
export function clockTime(at: string): string {
  const date = new Date(at);
  if (Number.isNaN(date.getTime())) {
    return at;
  }
  const two = (value: number) => String(value).padStart(2, "0");
  return `${two(date.getHours())}:${two(date.getMinutes())}:${two(date.getSeconds())}`;
}

/** The four texts of a line as drawn: time, level word, target and message. */
export function lineFields(line: LogLine): [string, string, string, string] {
  return [clockTime(line.at), line.level.toUpperCase(), line.target, line.message];
}

/** One line as plain text, the way it is copied. */
export function formatLine(line: LogLine): string {
  const [time, level, target, message] = lineFields(line);
  return `${time} ${level} ${target}: ${message}`;
}

/** Whether any part of the line contains `query`, ignoring case; an empty query matches. */
export function lineMatches(line: LogLine, query: string): boolean {
  const wanted = query.trim().toLowerCase();
  return wanted === "" || lineFields(line).some((part) => part.toLowerCase().includes(wanted));
}

/** `text` cut into runs, each marked when it is a match of `query` (ignoring case). */
export function splitMatches(text: string, query: string): { match: boolean; text: string }[] {
  const wanted = query.trim();
  if (wanted === "") {
    return [{ match: false, text }];
  }
  const escaped = wanted.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return text
    .split(new RegExp(`(${escaped})`, "i"))
    .map((part, index) => ({ match: index % 2 === 1, text: part }))
    .filter((part) => part.text !== "");
}
