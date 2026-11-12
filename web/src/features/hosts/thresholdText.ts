/** One threshold value as short text: lists joined with commas, never JSON. */
function valueText(value: unknown): string {
  if (Array.isArray(value)) {
    return value.map(valueText).join(", ");
  }
  if (value !== null && typeof value === "object") {
    return Object.entries(value)
      .map(([name, inner]) => `${name} ${valueText(inner)}`)
      .join(", ");
  }
  return String(value);
}

/**
 * A check's thresholds as plain words, `warning 85 · critical 95`: names as the check
 * knows them, values as text and lists joined with commas.
 */
export function thresholdWords(thresholds: Record<string, unknown>): string {
  return Object.entries(thresholds)
    .map(([name, value]) => `${name} ${valueText(value)}`)
    .join(" · ");
}
