const UNITS: [string, number][] = [
  ["d", 86_400],
  ["h", 3_600],
  ["m", 60],
];

/** How long ago `at` was, in the shortest unit that is at least one: "3 d", "5 h", "12 m". */
export function shortSpan(at: Date, now: Date): string {
  const seconds = Math.max(0, Math.round((now.getTime() - at.getTime()) / 1000));
  for (const [unit, size] of UNITS) {
    if (seconds >= size) {
      return `${Math.floor(seconds / size)} ${unit}`;
    }
  }
  return "under 1 m";
}
