import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Link, RouterProvider, createMemoryRouter } from "react-router";
import { ToastProvider } from "../../components";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { nas, T0 } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { NoteEditorPage } from "./NoteEditorPage";
import { useLeaveGuard } from "./useLeaveGuard";

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
  route("GET /api/v1/notes/{id}", () => note("n1", "Backups", "restic nightly"));
});

function renderEditor() {
  return renderAt("/notes/n1", "/notes/:id", <NoteEditorPage />);
}

test("notes_editor_previews_markdown_without_raw_html", async () => {
  const user = userEvent.setup();
  route("POST /api/v1/notes", ({ body }) => ({ ...note("n9", "x"), ...(body as object) }));
  const { container } = renderAt("/notes/new", "/notes/:id", <NoteEditorPage />);

  await user.type(await screen.findByLabelText(/^Title/), "Runbook");
  fireEvent.change(screen.getByLabelText("Body (Markdown)"), {
    target: { value: "**bold** step\n\n<script>alert(1)</script>\n\n<b>raw</b>" },
  });
  const preview = screen.getByRole("region", { name: "Preview" });
  expect(await within(preview).findByText("bold")).toBeVisible();
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

test("notes_editor_save_says_why_it_is_disabled_until_there_is_a_title", async () => {
  renderAt("/notes/new", "/notes/:id", <NoteEditorPage />);

  const save = await screen.findByRole("button", { name: "Save" });
  expect(save).toHaveAttribute("aria-disabled", "true");
  expect(save).toHaveAccessibleDescription("Give the note a title first.");
});

test("notes_editor_leaving_with_unsaved_changes_asks_first", async () => {
  const user = userEvent.setup();
  renderEditor();
  const title = await screen.findByLabelText(/^Title/);
  expect(screen.queryByText("Unsaved changes")).toBeNull();

  await user.type(title, " changed");
  expect(screen.getByText("Unsaved changes")).toBeVisible();
  await user.click(screen.getByRole("link", { name: "Back to notes" }));

  const dialog = screen.getByRole("dialog", { name: "Leave without saving?" });
  expect(screen.getByTestId("location"), "still on the note").toHaveTextContent("/notes/n1");
  await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(screen.getByLabelText(/^Title/)).toHaveValue("Backups changed");

  await user.click(screen.getByRole("link", { name: "Back to notes" }));
  await user.click(
    within(screen.getByRole("dialog")).getByRole("button", { name: "Leave without saving" }),
  );
  expect(await screen.findByText("other page")).toBeVisible();
  expect(screen.getByTestId("location")).toHaveTextContent("/notes");
});

test("notes_editor_leaves_without_a_question_when_nothing_changed", async () => {
  const user = userEvent.setup();
  renderEditor();
  await screen.findByLabelText(/^Title/);

  await user.click(screen.getByRole("link", { name: "Back to notes" }));

  expect(await screen.findByText("other page")).toBeVisible();
});

test("notes_editor_asks_the_browser_before_the_tab_is_closed_with_unsaved_changes", async () => {
  const user = userEvent.setup();
  renderEditor();
  const closing = () => {
    const event = new Event("beforeunload", { cancelable: true });
    window.dispatchEvent(event);
    return event.defaultPrevented;
  };
  const title = await screen.findByLabelText(/^Title/);
  expect(closing()).toBe(false);

  await user.type(title, "!");

  expect(closing()).toBe(true);
});

test("notes_editor_ctrl_s_saves_and_clears_the_unsaved_marker", async () => {
  const user = userEvent.setup();
  route("PUT /api/v1/notes/{id}", ({ body }) => ({ ...note("n1", "x"), ...(body as object) }));
  renderEditor();
  await user.type(await screen.findByLabelText(/^Title/), " 2");
  expect(screen.getByText("Unsaved changes")).toBeVisible();

  await user.keyboard("{Control>}s{/Control}");

  expect(await screen.findByText("Note saved")).toBeVisible();
  const put = calls.find((c) => c.route === "PUT /api/v1/notes/{id}");
  expect(put?.init.params?.path).toEqual({ id: "n1" });
  expect(put?.init.body).toMatchObject({ title: "Backups 2", body_markdown: "restic nightly" });
  expect(screen.queryByText("Unsaved changes")).toBeNull();
});

test("notes_editor_save_failure_stays_beside_the_form_in_words", async () => {
  const user = userEvent.setup();
  route("PUT /api/v1/notes/{id}", () => failure(400, "bad_request", "the note is too long"));
  renderEditor();
  await user.type(await screen.findByLabelText(/^Title/), "!");

  await user.click(screen.getByRole("button", { name: "Save" }));

  const alert = await screen.findByRole("alert");
  expect(alert).toHaveTextContent("Could not save the note");
  expect(alert).toHaveTextContent("The note is too long.");
  expect(screen.getByText("Unsaved changes")).toBeVisible();
});

test("notes_editor_delete_asks_naming_the_note_and_then_returns_to_the_list", async () => {
  const user = userEvent.setup();
  route("DELETE /api/v1/notes/{id}", () => undefined);
  renderEditor();
  await user.click(await screen.findByRole("button", { name: "Delete" }));

  const dialog = screen.getByRole("dialog", { name: "Delete “Backups”?" });
  await user.click(within(dialog).getByRole("button", { name: "Delete note" }));

  expect(await screen.findByText("Note deleted")).toBeVisible();
  expect(screen.getByTestId("location")).toHaveTextContent("/notes");
});

test("notes_editor_missing_note_says_so_with_a_way_back", async () => {
  route("GET /api/v1/notes/{id}", () => failure(404, "not_found", "no such note"));
  renderEditor();

  expect(await screen.findByText("No such note")).toBeVisible();
  expect(screen.getByRole("link", { name: "Back to notes" })).toHaveAttribute("href", "/notes");
});

test("notes_editor_load_error_shows_retry", async () => {
  route("GET /api/v1/notes/{id}", () => failure(500, "internal", "boom"));
  renderEditor();

  expect(await screen.findByText("Could not load the note")).toBeVisible();
  expect(screen.getByRole("button", { name: "Retry" })).toBeVisible();
});

function GuardedPage({ dirty }: { dirty: boolean }) {
  const { guard } = useLeaveGuard(dirty);
  return (
    <>
      <Link to="/elsewhere">Go elsewhere</Link>
      {guard}
    </>
  );
}

test("leave_guard_holds_a_data_router_navigation_until_the_person_confirms", async () => {
  const user = userEvent.setup();
  const router = createMemoryRouter(
    [
      { path: "/here", element: <GuardedPage dirty /> },
      { path: "/elsewhere", element: <p>the other page</p> },
    ],
    { initialEntries: ["/here"] },
  );
  render(
    <QueryClientProvider client={new QueryClient()}>
      <ToastProvider>
        <RouterProvider router={router} />
      </ToastProvider>
    </QueryClientProvider>,
  );

  await user.click(screen.getByRole("link", { name: "Go elsewhere" }));
  const dialog = await screen.findByRole("dialog", { name: "Leave without saving?" });
  expect(router.state.location.pathname).toBe("/here");

  await user.click(within(dialog).getByRole("button", { name: "Leave without saving" }));
  expect(await screen.findByText("the other page")).toBeVisible();
});
