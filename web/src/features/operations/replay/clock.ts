/**
 * A moment's time of day in the viewer's locale and zone, to the second. The locale and zone
 * default to the browser's; a caller names them only to pin the result.
 */
export function formatClock(atMs: number, locale?: string, timeZone?: string): string {
  return new Intl.DateTimeFormat(locale, {
    timeStyle: "medium",
    ...(timeZone === undefined ? {} : { timeZone }),
  }).format(atMs);
}
