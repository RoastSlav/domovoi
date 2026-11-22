/** Average width of one character at 12 px, in layout units: SVG text does not wrap or clip itself. */
export const CHAR_W = 6.3;

/** Cuts `text` to `max` characters with an ellipsis; empty when there is no room. */
export function clip(text: string, max: number): string {
  if (max <= 0) {
    return "";
  }
  return text.length <= max ? text : `${text.slice(0, Math.max(0, max - 1)).trimEnd()}…`;
}

/**
 * Wraps `text` into at most `lines` lines of `max` characters on word boundaries; the last
 * line ends in an ellipsis when text was left out. Words longer than a line are cut.
 */
export function wrapLines(text: string, max: number, lines: number): string[] {
  const words = text.split(/\s+/).filter(Boolean);
  const out: string[] = [];
  let line = "";
  let index = 0;
  while (index < words.length && out.length < lines) {
    const word = words[index] ?? "";
    const next = line ? `${line} ${word}` : word;
    if (next.length <= max) {
      line = next;
      index += 1;
    } else if (line) {
      out.push(line);
      line = "";
    } else {
      out.push(word.slice(0, max));
      index += 1;
    }
  }
  if (line && out.length < lines) {
    out.push(line);
  }
  if (index < words.length && out.length > 0) {
    const last = out.length - 1;
    out[last] = clip(`${out[last] ?? ""} ${words.slice(index).join(" ")}`, max);
  }
  return out;
}
