/** The longest excerpt kept; the table clamps it to two lines anyway. */
const EXCERPT_CHARS = 240;

/** The hub's own bookkeeping tags: they mark generated notes, and read as one word. */
const BOOKKEEPING_TAGS = ["generated", "host-profile"];

/** The tags worth showing: the person's own, without the hub's bookkeeping. */
export function visibleTags(tags: readonly string[]): string[] {
  return tags.filter((tag) => !BOOKKEEPING_TAGS.includes(tag));
}

/** A plain-text taste of a Markdown body: no fences, headings, links or emphasis marks. */
export function excerptOf(markdown: string): string {
  const plain = markdown
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/^\s{0,3}(#{1,6}|>|[-*+]|\d+\.)\s+/gm, "")
    .replace(/[*_`~]/g, "")
    .replace(/\s+/g, " ")
    .trim();
  return plain.length > EXCERPT_CHARS ? `${plain.slice(0, EXCERPT_CHARS).trimEnd()}…` : plain;
}
