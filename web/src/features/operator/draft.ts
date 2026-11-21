/**
 * The unsent question of each Operator conversation, kept in the browser so closing the panel or
 * a failed send loses nothing. A per-viewer convenience only: blocked storage degrades to memory.
 */

const memory = new Map<string, string>();

function key(id: string): string {
  return `operator:draft:${id}`;
}

/** The saved draft of conversation `id`, or an empty string. */
export function readDraft(id: string): string {
  try {
    return localStorage.getItem(key(id)) ?? memory.get(id) ?? "";
  } catch {
    return memory.get(id) ?? "";
  }
}

/** Saves `text` as the draft of conversation `id`; empty text removes it. */
export function writeDraft(id: string, text: string): void {
  if (text === "") {
    clearDraft(id);
    return;
  }
  memory.set(id, text);
  try {
    localStorage.setItem(key(id), text);
  } catch {
    // Storage is blocked or full; the in-memory copy still serves this tab.
  }
}

/** Forgets the draft of conversation `id`. */
export function clearDraft(id: string): void {
  memory.delete(id);
  try {
    localStorage.removeItem(key(id));
  } catch {
    // Nothing stored that could be removed.
  }
}
