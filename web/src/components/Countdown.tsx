import { useEffect, useState } from "react";
import { Tooltip } from "./Tooltip";
import styles from "./data.module.css";

function plural(n: number, unit: string): string {
  return `${n} ${unit}${n === 1 ? "" : "s"}`;
}

/**
 * Time left in the unit that fits it: "1 day 3 hours", "18 hours 5 minutes", "57 minutes",
 * then "m:ss" for the last ten minutes, where seconds matter.
 */
export function formatRemaining(ms: number): string {
  const total = Math.max(0, Math.ceil(ms / 1000));
  const days = Math.floor(total / 86400);
  const hours = Math.floor((total % 86400) / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  if (days > 0) {
    return hours > 0 ? `${plural(days, "day")} ${plural(hours, "hour")}` : plural(days, "day");
  }
  if (hours > 0) {
    return minutes > 0
      ? `${plural(hours, "hour")} ${plural(minutes, "minute")}`
      : plural(hours, "hour");
  }
  if (total > 600) {
    return plural(Math.ceil(total / 60), "minute");
  }
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
}

/** Milliseconds until `until`, updated every second; 0 once it has passed. */
export function useRemaining(until: string): number {
  const end = new Date(until).getTime();
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, []);
  return Math.max(0, end - now);
}

/**
 * Time left until `until` as "m:ss", with the exact end time as its tooltip. The caller ticks
 * it with `useRemaining`.
 *
 * Use it for a deadline minutes away (an approval window, a login window).
 */
export function Countdown({ remaining, until }: { remaining: number; until: string }) {
  return (
    <Tooltip content={new Date(until).toLocaleString()}>
      <time dateTime={until} className={styles.time}>
        {formatRemaining(remaining)}
      </time>
    </Tooltip>
  );
}
