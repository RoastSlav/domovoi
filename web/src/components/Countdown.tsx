import { useEffect, useState } from "react";

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

/** Time left until `until`, ticking each second, as a `<time>` element. */
export function Countdown({ remaining, until }: { remaining: number; until: string }) {
  return (
    <time dateTime={until} title={new Date(until).toLocaleString()}>
      {formatRemaining(remaining)}
    </time>
  );
}
