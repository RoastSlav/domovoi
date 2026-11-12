import type { Note, NoteInput } from "../../api/types";

/** What the form edits. */
export interface Draft {
  body: string;
  host: string;
  tags: string;
  title: string;
}

export function draftOf(note: Note | undefined, host: string): Draft {
  return {
    title: note?.title ?? "",
    host,
    tags: note?.tags.join(", ") ?? "",
    body: note?.body_markdown ?? "",
  };
}

export function inputOf(draft: Draft): NoteInput {
  const tags = draft.tags
    .split(",")
    .map((tag) => tag.trim())
    .filter(Boolean);
  return {
    title: draft.title.trim(),
    body_markdown: draft.body,
    tags,
    ...(draft.host ? { host: draft.host } : {}),
  };
}

export const sameDraft = (a: Draft, b: Draft) =>
  a.title === b.title && a.host === b.host && a.tags === b.tags && a.body === b.body;
