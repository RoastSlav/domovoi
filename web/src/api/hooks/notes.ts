import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "../client";
import { keys } from "../queryClient";
import {
  call,
  defined,
  type ImportResult,
  type Note,
  type NoteHit,
  type NoteInput,
} from "../types";

/** Notes about `host` (by name), or global notes when it is absent. */
export function useNotes(host: string | undefined) {
  return useQuery({
    queryKey: keys.noteList(host),
    queryFn: () => call<Note[]>(api.GET("/api/v1/notes", { params: { query: defined({ host }) } })),
  });
}

/** Search hits for `q`, best first; idle while `q` is blank. */
export function useNoteSearch(q: string, host: string | undefined) {
  const query = q.trim();
  return useQuery({
    queryKey: keys.noteSearch(query, host),
    queryFn: () =>
      call<NoteHit[]>(
        api.GET("/api/v1/notes/search", { params: { query: { q: query, ...defined({ host }) } } }),
      ),
    enabled: query !== "",
  });
}

/** One note; idle while `id` is empty. */
export function useNote(id: string) {
  return useQuery({
    queryKey: keys.note(id),
    queryFn: () => call<Note>(api.GET("/api/v1/notes/{id}", { params: { path: { id } } })),
    enabled: id !== "",
  });
}

function useRefreshNotes() {
  const client = useQueryClient();
  return () => client.invalidateQueries({ queryKey: keys.notes });
}

/** Creates a note. */
export function useCreateNote() {
  const onSuccess = useRefreshNotes();
  return useMutation({
    mutationFn: (body: NoteInput) => call<Note>(api.POST("/api/v1/notes", { body })),
    onSuccess,
  });
}

/** Replaces a note. */
export function useSaveNote() {
  const onSuccess = useRefreshNotes();
  return useMutation({
    mutationFn: ({ id, body }: { id: string; body: NoteInput }) =>
      call<Note>(api.PUT("/api/v1/notes/{id}", { params: { path: { id } }, body })),
    onSuccess,
  });
}

/** Deletes a note. */
export function useDeleteNote() {
  const onSuccess = useRefreshNotes();
  return useMutation({
    mutationFn: (id: string) =>
      call(api.DELETE("/api/v1/notes/{id}", { params: { path: { id } } })),
    onSuccess,
  });
}

/** What an import uploads. */
export interface ImportInput {
  files: File[];
  /** The host the notes are about, by name; absent for global notes. */
  host?: string | undefined;
}

/** The multipart form for `input`, with folder-relative names where the browser has them. */
export function importForm(input: ImportInput): FormData {
  const form = new FormData();
  if (input.host) {
    form.append("host", input.host);
  }
  for (const file of input.files) {
    form.append("files", file, file.webkitRelativePath || file.name);
  }
  return form;
}

/** Imports Markdown files as notes. */
export function useImportNotes() {
  const onSuccess = useRefreshNotes();
  return useMutation({
    mutationFn: (input: ImportInput) =>
      call<ImportResult>(
        api.POST("/api/v1/notes/import", {
          // The generated type describes the form's fields; the body is sent as built.
          body: { files: [] },
          bodySerializer: () => importForm(input),
        }),
      ),
    onSuccess,
  });
}
