import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, test, vi } from "vitest";
import type { ResolutionSummary as Summary } from "../../api/types";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import drafting from "../../test/fixtures/experience/summary-drafting.json";
import ready from "../../test/fixtures/experience/summary-ready.json";
import { nas } from "../../test/fixtures";
import { renderWithProfile } from "../../test/visibility";
import type { ProfileId } from "../../design/visibility";
import { ResolutionSummary } from "./ResolutionSummary";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const OP = "0190a0a0-0000-7000-8000-0000000000b1";
const HOST = "0190a0a0-0000-7000-8000-0000000000c1";
const ME = "0190a0a0-0000-7000-8000-0000000000d1";
const GET = "GET /api/v1/operations/{id}/summary";
const POST = "POST /api/v1/operations/{id}/summary/{field}";
const READY = ready as unknown as Summary;
const posts = () => calls.filter((call) => call.route === POST);

beforeEach(() => {
  resetApi();
  route("GET /api/v1/hosts", () => [{ ...nas, id: HOST }]);
  route("GET /api/v1/session", () => ({ csrf_token: "x", user: { id: ME, username: "owner" } }));
});

function show(summary: unknown, profile: ProfileId = "detailed") {
  route(GET, () => summary);
  return renderWithProfile(
    profile,
    `/operations/${OP}`,
    "/operations/:id",
    <ResolutionSummary operationId={OP} />,
  );
}

async function row(label: string) {
  const list = await screen.findByRole("list", { name: "Summary fields" });
  const item = within(list).getByText(label).closest("li");
  if (!item) {
    throw new Error(`no row ${label}`);
  }
  return within(item);
}

test("summary_states_not_ready_drafting_ready_and_failed", async () => {
  const view = show({ ...READY, state: "not_ready", fields: [], history: [] });
  await waitFor(() => expect(calls.filter((c) => c.route === GET)).toHaveLength(1));
  await waitFor(() => expect(screen.queryByText("Loading…")).toBeNull());
  expect(screen.queryByText("Resolution summary")).toBeNull();
  view.unmount();

  show(drafting);
  expect(await screen.findByText("Domovoi is writing the summaryâ€¦")).toBeVisible();
  expect(screen.getByRole("heading", { name: "Resolution summary" })).toBeVisible();
});

test("a_ready_summary_lists_each_field_with_its_source_and_a_failed_load_offers_retry", async () => {
  const view = show(READY);
  expect(await (await row("Problem")).findByText("plex was stopped")).toBeVisible();
  expect((await row("Problem")).getByText("Drafted by the model, not checked")).toBeVisible();
  expect((await row("Cause")).getByText("Written or confirmed by you")).toBeVisible();
  expect((await row("Cause")).getByText(/You,/)).toBeVisible();
  expect((await row("Hosts")).getByText("nas")).toBeVisible();
  expect((await row("Outcome")).getByText("Succeeded")).toBeVisible();
  view.unmount();

  resetApi();
  route("GET /api/v1/hosts", () => [{ ...nas, id: HOST }]);
  route("GET /api/v1/session", () => ({ csrf_token: "x", user: { id: ME, username: "owner" } }));
  show(failure(503, "unavailable", "orchestration is not enabled"));
  expect(await screen.findByRole("button", { name: "Retry" })).toBeVisible();
});

test("code_owned_fields_have_no_buttons_and_narrative_fields_show_confirm_and_edit", async () => {
  show(READY);
  for (const label of ["Outcome", "Fix verified", "What it was about", "What ran", "Hosts"]) {
    expect((await row(label)).queryAllByRole("button")).toHaveLength(0);
  }
  const problem = await row("Problem");
  expect(problem.getByRole("button", { name: "Confirm" })).toBeVisible();
  expect(problem.getByRole("button", { name: "Edit" })).toBeVisible();
  const cause = await row("Cause");
  expect(cause.queryByRole("button", { name: "Confirm" })).toBeNull();
  expect(cause.getByRole("button", { name: "Edit" })).toBeVisible();
});

test("confirm_sends_the_same_text_with_the_current_revision_and_says_nothing_changed_when_unchanged", async () => {
  const user = userEvent.setup();
  route(POST, () => ({ field: READY.fields[5], unchanged: false }));
  show(READY);
  await user.click((await row("Problem")).getByRole("button", { name: "Confirm" }));
  expect(await screen.findByText("Confirmed. Domovoi now treats it as your words.")).toBeVisible();
  expect(posts()).toHaveLength(1);
  expect(posts()[0]?.init.params?.path).toEqual({ id: OP, field: "problem" });
  expect(posts()[0]?.init.body).toEqual({ value: "plex was stopped", expected_revision: 1 });

  route(POST, () => ({ field: READY.fields[7], unchanged: true }));
  await user.click((await row("Fix")).getByRole("button", { name: "Confirm" }));
  expect(await screen.findByText("Nothing changed: this already is your text.")).toBeVisible();
});

test("edit_sends_the_new_text_and_revision_and_a_stale_edit_shows_inside_the_dialog_and_refetches", async () => {
  const user = userEvent.setup();
  route(POST, () => failure(409, "conflict", "someone changed it meanwhile; reload and try again"));
  show(READY);
  await user.click((await row("Cause")).getByRole("button", { name: "Edit" }));
  const dialog = await screen.findByRole("dialog");
  const box = within(dialog).getByRole("textbox");
  expect(box).toHaveValue("the disk filled up");
  await user.clear(box);
  await user.type(box, "a log filled the disk");
  const before = calls.filter((c) => c.route === GET).length;
  await user.click(within(dialog).getByRole("button", { name: "Save" }));
  expect(await within(dialog).findByText(/someone changed it meanwhile/i)).toBeVisible();
  expect(posts()[0]?.init.body).toEqual({ value: "a log filled the disk", expected_revision: 2 });
  await waitFor(() => expect(calls.filter((c) => c.route === GET).length).toBeGreaterThan(before));
  expect(within(screen.getByRole("dialog")).getByRole("textbox")).toHaveValue(
    "a log filled the disk",
  );
});

test("a_successful_edit_closes_the_dialog_and_toasts", async () => {
  const user = userEvent.setup();
  route(POST, () => ({ field: READY.fields[6], unchanged: false }));
  show(READY);
  await user.click((await row("Cause")).getByRole("button", { name: "Edit" }));
  const dialog = await screen.findByRole("dialog");
  await user.type(within(dialog).getByRole("textbox"), "!");
  await user.click(within(dialog).getByRole("button", { name: "Save" }));
  expect(await screen.findByText("Saved your edit.")).toBeVisible();
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
});

test("an_empty_or_too_long_edit_is_blocked_with_a_reason", async () => {
  const user = userEvent.setup();
  show(READY);
  await user.click((await row("Cause")).getByRole("button", { name: "Edit" }));
  const dialog = await screen.findByRole("dialog");
  const box = within(dialog).getByRole("textbox");
  const save = within(dialog).getByRole("button", { name: "Save" });
  await user.clear(box);
  expect(save).toHaveAttribute("aria-disabled", "true");
  await user.click(save);
  expect(posts()).toHaveLength(0);
  await user.click(box);
  await user.paste("x".repeat(1001));
  expect(save).toHaveAttribute("aria-disabled", "true");
  expect(within(dialog).getByText(/1001 of 1000/)).toBeVisible();
});

test("a_missing_narrative_field_can_be_added_with_revision_zero", async () => {
  const user = userEvent.setup();
  route(POST, () => ({ field: READY.fields[7], unchanged: false }));
  const withoutFix = { ...READY, fields: READY.fields.filter((f) => f.field !== "fix") };
  show(withoutFix);
  const list = await screen.findByRole("list", { name: "Summary fields" });
  await user.click(within(list).getByRole("button", { name: "Add the fix" }));
  const dialog = await screen.findByRole("dialog");
  await user.type(within(dialog).getByRole("textbox"), "restart plex");
  await user.click(within(dialog).getByRole("button", { name: "Save" }));
  await waitFor(() => expect(posts()).toHaveLength(1));
  expect(posts()[0]?.init.params?.path).toEqual({ id: OP, field: "fix" });
  expect(posts()[0]?.init.body).toEqual({ value: "restart plex", expected_revision: 0 });
});

test("summary_history_is_collapsed_in_standard_and_lists_revisions_with_sources", async () => {
  const user = userEvent.setup();
  const view = show(READY, "standard");
  await row("Problem");
  expect(screen.queryByRole("list", { name: "Revisions" })).toBeNull();
  const toggle = screen.getByRole("button", { name: /Edit history/ });
  await user.click(toggle);
  const revisions = within(await screen.findByRole("list", { name: "Revisions" }));
  expect(revisions.getAllByRole("listitem")).toHaveLength(READY.history.length);
  expect(revisions.getAllByText("Drafted by the model, not checked").length).toBeGreaterThan(0);
  view.unmount();
});

test("older_revisions_are_not_listed_says_so_when_the_history_is_cut", async () => {
  show({ ...READY, history_truncated: true });
  expect(await screen.findByText("Older revisions are not listed.")).toBeVisible();
});
