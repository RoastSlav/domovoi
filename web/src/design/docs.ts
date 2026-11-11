/**
 * The published user docs. The book a link points at can be ahead of the version running
 * here, and is unreachable when the hub is offline: the words on the page carry the meaning,
 * and the link is the way to the longer version.
 */
const DOCS = "https://RoastSlav.github.io/domovoi/";

/** The address of a page in the book; `path` is like `concepts/incidents.html`. */
export function docsUrl(path: string): string {
  return `${DOCS}${path}`;
}
