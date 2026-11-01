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

/** A timestamp shown relative to now, with the exact time on hover and for machines. */
export function RelativeTime({ at, now }: { at: string; now?: Date }) {
  const date = new Date(at);
  if (Number.isNaN(date.getTime())) {
    return <span>{at}</span>;
  }
  return (
    <time dateTime={at} title={date.toLocaleString()}>
      {relative(date, now ?? new Date())}
    </time>
  );
}
