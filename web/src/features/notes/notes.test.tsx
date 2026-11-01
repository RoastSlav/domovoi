import { act, fireEvent, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { calls, resetApi, route } from "../../test/apiMock";
import { nas, T0 } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { NoteEditorPage } from "./NoteEditorPage";
import { NotesPage, SEARCH_DEBOUNCE_MS } from "./NotesPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const note = (id: string, title: string, body = "") => ({
  id,
  host_id: null,
  title,
  body_markdown: body,
  tags: [],
  updated_at: T0,
});

beforeEach(() => {
  resetApi();
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/notes", () => [note("n1", "Backups")]);
});

afterEach(() => {
  vi.useRealTimers();
});

function searches() {
  return calls.filter((c) => c.route === "GET /api/v1/notes/search");
}

test("notes_search_debounces_and_renders_snippets", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  const user = userEvent.setup({ advanceTimers: (ms) => vi.advanceTimersByTime(ms) });
  route("GET /api/v1/notes/search", ({ params }) => [
    {
      note: note("n2", "ZFS pools"),
      snippet: `…scrub the ${String(params?.query?.q)} pool monthly…`,
      score: 2.5,
    },
  ]);
  renderAt("/notes", "/notes", <NotesPage />);
  expect(await screen.findByRole("link", { name: "Backups" })).toBeVisible();

  const box = screen.getByLabelText("Search");
  fireEvent.change(box, { target: { value: "z" } });
  await act(() => vi.advanceTimersByTimeAsync(100));
  fireEvent.change(box, { target: { value: "zf" } });
  await act(() => vi.advanceTimersByTimeAsync(100));
  fireEvent.change(box, { target: { value: "zfs" } });
  // The fake clock also follows real time (`shouldAdvanceTime`), so check well before the
  // debounce ends rather than 1 ms before it, which a slow runner can overshoot.
  await act(() => vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS / 2));
  expect(searches(), "nothing while typing").toHaveLength(0);

  await act(() => vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS / 2));
  const results = await screen.findByRole("list", { name: "Search results" });
  expect(searches().map((c) => c.init.params?.query)).toEqual([{ q: "zfs" }]);
  expect(within(results).getByRole("link", { name: "ZFS pools" })).toHaveAttribute(
    "href",
    "/notes/n2",
  );
  expect(within(results).getByText("…scrub the zfs pool monthly…")).toBeVisible();

  await user.clear(box);
  await act(() => vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS));
  expect(await screen.findByRole("list", { name: "Notes" })).toBeVisible();
});

test("notes_editor_previews_markdown_without_raw_html", async () => {
  const user = userEvent.setup();
  route("POST /api/v1/notes", ({ body }) => ({ ...note("n9", "x"), ...(body as object) }));
  const { container } = renderAt("/notes/new", "/notes/:id", <NoteEditorPage />);

  await user.type(await screen.findByLabelText("Title"), "Runbook");
  const body = screen.getByLabelText("Body (Markdown)");
  fireEvent.change(body, {
    target: { value: "**bold** step\n\n<script>alert(1)</script>\n\n<b>raw</b>" },
  });
  const preview = screen.getByRole("region", { name: "Preview" });
  expect(within(preview).getByText("bold").tagName).toBe("STRONG");
  expect(container.querySelector("script"), "no script element anywhere").toBeNull();
  expect(preview.querySelector("b"), "no raw HTML element").toBeNull();

  await user.click(screen.getByRole("button", { name: "Save" }));
  expect(calls.find((c) => c.route === "POST /api/v1/notes")?.init.body).toEqual({
    title: "Runbook",
    body_markdown: "**bold** step\n\n<script>alert(1)</script>\n\n<b>raw</b>",
    tags: [],
  });
  expect(await screen.findByTestId("location")).toHaveTextContent("/notes/n9");
});

test("notes_import_uploads_files_and_reports_counts", async () => {
  const user = userEvent.setup({ applyAccept: false });
  route("POST /api/v1/notes/import", () => ({
    created: 1,
    skipped: 1,
    skipped_files: [{ name: "old.md", reason: 'a note titled "Old" already exists' }],
  }));
  renderAt("/notes?host=nas", "/notes", <NotesPage />);
  await user.click(await screen.findByRole("button", { name: "Import" }));
  const dialog = screen.getByRole("dialog", { name: "Import notes" });
  expect(within(dialog).getByLabelText("Notes about")).toHaveValue("nas");

  await user.upload(within(dialog).getByLabelText("Or pick files"), [
    new File(["# ZFS"], "zfs.md", { type: "text/markdown" }),
    new File(["# Old"], "old.md", { type: "text/markdown" }),
    new File(["png"], "diagram.png", { type: "image/png" }),
  ]);
  expect(within(dialog).getByText("2 Markdown files selected.")).toBeVisible();
  await user.click(within(dialog).getByRole("button", { name: "Import 2 files" }));

  expect(await within(dialog).findByText("Created 1 note, skipped 1.")).toBeVisible();
  expect(within(dialog).getByRole("list", { name: "Skipped files" })).toHaveTextContent(
    'old.md: a note titled "Old" already exists',
  );
  const sent = calls.find((c) => c.route === "POST /api/v1/notes/import");
  const form = sent?.init.bodySerializer?.(sent.init.body) as FormData;
  expect(form.get("host")).toBe("nas");
  expect(form.getAll("files").map((f) => (f as File).name)).toEqual(["zfs.md", "old.md"]);
});
