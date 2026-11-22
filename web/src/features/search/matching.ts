/** The distinct words of a search text, lower-cased; none when the text is blank. */
export function words(query: string): string[] {
  return [...new Set(query.toLowerCase().split(/\s+/).filter(Boolean))];
}

/**
 * Whether every word occurs in `text`. Case is ignored and characters are literal, so
 * `a.b` matches only "a.b" and never "axb". No words match nothing.
 */
export function matchesAll(text: string, wanted: readonly string[]): boolean {
  if (wanted.length === 0) {
    return false;
  }
  const lower = text.toLowerCase();
  return wanted.every((word) => lower.includes(word));
}
