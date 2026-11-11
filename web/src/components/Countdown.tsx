import { useEffect, useState } from "react";
import { Tooltip } from "./Tooltip";
import styles from "./data.module.css";

/** `ms` as "m:ss", or "h:mm:ss" from an hour up. */
export function formatRemaining(ms: number): string {
  const total = Math.max(0, Math.ceil(ms / 1000));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = String(total % 60).padStart(2, "0");
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, "0")}:${seconds}`
    : `${minutes}:${seconds}`;
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
