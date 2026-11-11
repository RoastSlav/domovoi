import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { nas, T0 } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { NotesPage } from "./NotesPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const ZONE = "Choose Markdown files or drop them here";

beforeEach(() => {
  resetApi();
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/notes", () => [
    { id: "n1", host_id: null, title: "Backups", body_markdown: "", tags: [], updated_at: T0 },
  ]);
});

async function openImport(user: ReturnType<typeof userEvent.setup>) {
  renderAt("/notes?host=nas", "/notes", <NotesPage />);
  await user.click(await screen.findByRole("button", { name: "Import" }));
  return screen.getByRole("dialog", { name: "Import notes" });
}

const files = [
  new File(["# ZFS"], "zfs.md", { type: "text/markdown" }),
  new File(["# Old"], "old.md", { type: "text/markdown" }),
  new File(["png"], "diagram.png", { type: "image/png" }),
];

test("notes_import_uploads_markdown_files_and_summarises_the_counts", async () => {
  const user = userEvent.setup({ applyAccept: false });
  route("POST /api/v1/notes/import", () => ({
    created: 1,
    skipped: 1,
    skipped_files: [{ name: "old.md", reason: 'a note titled "Old" already exists' }],
  }));
  const dialog = await openImport(user);
  expect(within(dialog).getByLabelText("Notes about")).toHaveValue("nas");

  await user.upload(within(dialog).getByLabelText(ZONE), files);
  expect(within(dialog).getByText("2 Markdown files selected.")).toBeVisible();
  await user.click(within(dialog).getByRole("button", { name: "Import 2 files" }));

  const summary = await within(dialog).findByText("1 note imported");
  expect(summary).toBeVisible();
  expect(within(dialog).getByText("1 file skipped")).toBeVisible();
  expect(within(dialog).getByRole("list", { name: "Skipped files" })).toHaveTextContent(
    'old.md: a note titled "Old" already exists',
  );
  const sent = calls.find((c) => c.route === "POST /api/v1/notes/import");
  const form = sent?.init.bodySerializer?.(sent.init.body) as FormData;
  expect(form.get("host")).toBe("nas");
  expect(form.getAll("files").map((f) => (f as File).name)).toEqual(["zfs.md", "old.md"]);
});

test("notes_import_with_no_files_says_what_to_pick_first", async () => {
  const user = userEvent.setup();
  const dialog = await openImport(user);

  const button = within(dialog).getByRole("button", { name: "Import 0 files" });

  expect(button).toHaveAttribute("aria-disabled", "true");
  expect(button).toHaveAccessibleDescription("Pick Markdown files first.");
});

test("notes_import_failure_is_shown_in_words_inside_the_dialog", async () => {
  const user = userEvent.setup({ applyAccept: false });
  route("POST /api/v1/notes/import", () => failure(400, "bad_request", "too many files"));
  const dialog = await openImport(user);
  await user.upload(within(dialog).getByLabelText(ZONE), files);

  await user.click(within(dialog).getByRole("button", { name: "Import 2 files" }));

  const alert = await within(dialog).findByRole("alert");
  expect(alert).toHaveTextContent("Could not import the notes");
  expect(within(dialog).queryByText(/imported$/)).toBeNull();
});
