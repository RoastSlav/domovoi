import { act, fireEvent, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { nas, T0 } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { NotesPage, SEARCH_DEBOUNCE_MS } from "./NotesPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const note = (id: string, title: string, body = "") => ({
  id,
  host_id: null,
  title,
  body_markdown: body,
  tags: [] as string[],
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

test("notes_page_explains_that_triage_reads_the_notes", async () => {
  renderAt("/notes", "/notes", <NotesPage />);

  expect(await screen.findByText(/^Runbooks in Markdown:/)).toBeVisible();
  expect(
    screen.getByText(/Triage searches them and gives the relevant ones to the model\./),
  ).toBeVisible();
  expect(
    screen.getByRole("link", { name: "Learn more about notes (opens in a new tab)" }),
  ).toHaveAttribute(
    "href",
    "https://RoastSlav.github.io/domovoi/guide/migrating.html#4-bring-your-notes",
  );
});

test("notes_list_shows_scope_excerpt_and_tags_for_each_note", async () => {
  route("GET /api/v1/notes", () => [
    {
      ...note("n1", "Backups", "# Backups\n\nRun **restic** nightly to the `nas` share."),
      host_id: nas.id,
      tags: ["backup"],
    },
  ]);
  renderAt("/notes", "/notes", <NotesPage />);

  const table = await screen.findByRole("table", { name: "Notes" });
  expect(within(table).getByRole("link", { name: "Backups" })).toHaveAttribute("href", "/notes/n1");
  expect(within(table).getByText("Backups Run restic nightly to the nas share.")).toBeVisible();
  expect(await within(table).findByText(nas.name)).toBeVisible();
  expect(within(table).getByText("backup")).toBeVisible();
});

test("notes_search_debounces_and_marks_the_matching_words", async () => {
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
  const results = await screen.findByRole("table", { name: "Search results" });
  expect(searches().map((c) => c.init.params?.query)).toEqual([{ q: "zfs" }]);
  expect(within(results).getByRole("link", { name: "ZFS pools" })).toHaveAttribute(
    "href",
    "/notes/n2",
  );
  expect(within(results).getByText("…scrub the", { exact: false })).toHaveTextContent(
    "…scrub the zfs pool monthly…",
  );
  const marked = [...results.querySelectorAll("mark")].map((mark) => mark.textContent);
  expect(marked, "in the title and in the snippet").toEqual(["ZFS", "zfs"]);

  await user.clear(box);
  expect(await screen.findByRole("table", { name: "Notes" })).toBeVisible();
});

test("notes_search_without_matches_offers_clear_search_and_is_not_the_empty_list", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/notes/search", () => []);
  renderAt("/notes", "/notes", <NotesPage />);
  await screen.findByRole("link", { name: "Backups" });

  await user.type(screen.getByLabelText("Search"), "nothing");

  expect(await screen.findByText("No notes match “nothing”")).toBeVisible();
  expect(screen.queryByText("No notes here yet")).toBeNull();
  await user.click(screen.getByRole("button", { name: "Clear search" }));
  expect(screen.getByLabelText("Search")).toHaveValue("");
  expect(await screen.findByRole("table", { name: "Notes" })).toBeVisible();
});

test("notes_page_with_no_notes_says_how_to_start", async () => {
  route("GET /api/v1/notes", () => []);
  renderAt("/notes", "/notes", <NotesPage />);

  expect(await screen.findByText("No notes here yet")).toBeVisible();
  expect(screen.getByText("Write one, or import a folder of Markdown runbooks.")).toBeVisible();
});

test("notes_list_load_error_shows_retry_instead_of_an_empty_list", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/notes", () => failure(500, "internal", "boom"));
  renderAt("/notes", "/notes", <NotesPage />);

  const alert = await screen.findByText("Could not load notes");
  expect(screen.queryByText("No notes here yet")).toBeNull();

  route("GET /api/v1/notes", () => [note("n1", "Backups")]);
  await user.click(
    within(alert.closest('[role="alert"]') as HTMLElement).getByRole("button", { name: "Retry" }),
  );
  expect(await screen.findByRole("link", { name: "Backups" })).toBeVisible();
});

test("notes_host_scope_load_error_shows_retry_instead_of_only_global", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/hosts", () => failure(500, "internal", "boom"));
  renderAt("/notes", "/notes", <NotesPage />);

  const message = await screen.findByText("Could not load hosts");
  expect(screen.queryByLabelText("Notes about")).toBeNull();

  route("GET /api/v1/hosts", () => [nas]);
  await user.click(
    within(message.closest('[role="alert"]') as HTMLElement).getByRole("button", { name: "Retry" }),
  );
  expect(await screen.findByLabelText("Notes about")).toBeVisible();
});

test("notes_page_starts_on_the_selected_host_and_a_pick_here_overrides_it", async () => {
  const user = userEvent.setup();
  renderAt("/notes?host=nas", "/notes", <NotesPage />);
  const scope = await screen.findByLabelText("Notes about");
  await screen.findByRole("option", { name: "nas" });
  expect(scope).toHaveValue("nas");
  const noteQueries = () =>
    calls.filter((c) => c.route === "GET /api/v1/notes").map((c) => c.init.params?.query);
  expect(noteQueries().at(-1)).toEqual({ host: "nas" });

  await user.selectOptions(scope, "Global");

  expect(scope).toHaveValue("");
  await screen.findByRole("table", { name: "Notes" });
  expect(noteQueries().at(-1) ?? {}).not.toHaveProperty("host");
  expect(screen.getByTestId("location"), "the sidebar's choice is untouched").toHaveTextContent(
    "/notes?host=nas",
  );
});

test("notes_page_marks_generated_notes_rather_than_their_hub_tags", async () => {
  route("GET /api/v1/notes", () => [
    note("n1", "Backups"),
    { ...note("gp", "nas"), tags: ["generated", "host-profile"] },
  ]);
  renderAt("/notes", "/notes", <NotesPage />);
  const list = await screen.findByRole("table", { name: "Notes" });
  expect(within(list).getByText("Generated")).toBeVisible();
  expect(within(list).queryByText("host-profile")).toBeNull();
});
