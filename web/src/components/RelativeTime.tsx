import { Tooltip } from "./Tooltip";
import styles from "./data.module.css";

const UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ["day", 86_400],
  ["hour", 3_600],
  ["minute", 60],
];

const format = new Intl.RelativeTimeFormat("en", { numeric: "auto" });

/** `at` relative to `now`, for example "5 minutes ago". */
export function relative(at: Date, now: Date): string {
  const seconds = Math.round((at.getTime() - now.getTime()) / 1000);
  for (const [unit, size] of UNITS) {
    if (Math.abs(seconds) >= size) {
      return format.format(Math.round(seconds / size), unit);
    }
  }
  return "just now";
}

/**
 * A timestamp shown relative to now ("5 minutes ago"), with the exact local time as its
 * tooltip and the ISO time for machines.
 *
 * Use it for every time the person reads at a glance. Where the exact time is the point
 * (a log line), print the time itself.
 */
export function RelativeTime({ at, now }: { at: string; now?: Date }) {
  const date = new Date(at);
  if (Number.isNaN(date.getTime())) {
    return <span>{at}</span>;
  }
  return (
    <Tooltip content={date.toLocaleString()}>
      <time dateTime={at} className={styles.time}>
        {relative(date, now ?? new Date())}
      </time>
    </Tooltip>
  );
}
