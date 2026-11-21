import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { failure, route } from "../../test/apiMock";
import { PREVIEW, SAVE, changeNote, hub, made, setup, type User } from "../../test/draftPage";
import { DRAFT_4, PREVIEW_BLOCKING } from "../../test/policyFixtures";
import type { PolicyDraftSaved, PolicyRule } from "../../api/types";

vi.mock("../../api/client", () => import("../../test/apiMock"));

// Every test types and clicks through several dialogs; the default 5 s is tight on a busy machine.
vi.setConfig({ testTimeout: 30_000 });

type SaveBody = {
  base_revision: number;
  password?: string;
  preview_hash?: string;
  rules: PolicyRule[];
};

/** Makes the page dirty, answers a check with locked changes, and opens the password dialog. */
async function openLockedDialog(user: User) {
  setup();
  hub.preview = PREVIEW_BLOCKING;
  await changeNote(user, "ask-containers", "Ask first");
  await user.click(screen.getByRole("button", { name: "Save draft" }));
  return screen.findByRole("dialog", { name: "Change a locked rule" });
}

async function enterPassword(user: User, dialog: HTMLElement, password: string) {
  await user.type(within(dialog).getByLabelText("Password"), password);
  await user.click(within(dialog).getByRole("button", { name: "Save draft" }));
}

test("saving_without_locked_changes_previews_then_saves_with_no_password", async () => {
  const user = userEvent.setup();
  setup();
  await changeNote(user, "ask-containers", "Ask first");
  await user.click(screen.getByRole("button", { name: "Save draft" }));

  expect(await screen.findByText("Draft saved.")).toBeVisible();
  expect(made(PREVIEW)).toHaveLength(1);
  const saves = made(SAVE);
  expect(saves).toHaveLength(1);
  const body = saves[0]?.init.body as SaveBody;
  expect(body.base_revision).toBe(4);
  expect(body).not.toHaveProperty("password");
  expect(body).not.toHaveProperty("preview_hash");
  expect(body.rules.find((rule) => rule.id === "ask-containers")?.note).toBe("Ask first");
  expect(await screen.findByText("Revision 5")).toBeVisible();
  expect(screen.queryByText("Unsaved changes")).toBeNull();
  expect(screen.queryByText("This draft changed elsewhere")).toBeNull();
  expect(screen.getByRole("link", { name: "Review and publish" })).toBeVisible();
});

test("a_failed_check_before_saving_stops_the_save_and_shows_the_problems", async () => {
  const user = userEvent.setup();
  setup();
  route(PREVIEW, () =>
    failure(400, "invalid", "invalid rules", undefined, [
      { path: "rules.ask-containers", message: "Unknown category.", severity: "error" },
    ]),
  );
  await changeNote(user, "ask-containers", "Ask first");
  await user.click(screen.getByRole("button", { name: "Save draft" }));
  expect(await screen.findByText("Could not check the rules")).toBeVisible();
  expect((await screen.findAllByText(/Unknown category\./)).length).toBeGreaterThan(0);
  expect(made(SAVE)).toHaveLength(0);
  expect(screen.getByText("Unsaved changes")).toBeVisible();
});

test("saving_a_locked_change_shows_the_changes_the_simulation_and_asks_for_the_password", async () => {
  const user = userEvent.setup();
  const dialog = await openLockedDialog(user);
  expect(within(dialog).getByText("Locked rule added")).toBeVisible();
  expect(within(dialog).getByText("no-prune")).toBeVisible();
  expect(within(dialog).getByText(/absolute prohibition/)).toBeVisible();
  expect(within(dialog).getByText("Would become Allow automatically")).toBeVisible();
  expect(
    within(dialog).getByText("2 recorded decisions would become Allow automatically."),
  ).toBeVisible();
  expect(made(SAVE)).toHaveLength(0);

  await enterPassword(user, dialog, "correct horse");
  expect(await screen.findByText("Draft saved. 1 locked rule change was recorded.")).toBeVisible();
  const body = made(SAVE)[0]?.init.body as SaveBody;
  expect(body.password).toBe("correct horse");
  expect(body.preview_hash).toBe(PREVIEW_BLOCKING.preview_hash);
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("the_password_is_never_kept_after_a_save", async () => {
  const user = userEvent.setup();
  const dialog = await openLockedDialog(user);
  await enterPassword(user, dialog, "correct horse");
  await screen.findByText(/Draft saved/);
  expect(document.body.innerHTML).not.toContain("correct horse");
  expect(window.location.href).not.toContain("correct");
});

test("a_wrong_password_is_a_field_error_and_the_dialog_stays", async () => {
  const user = userEvent.setup();
  const dialog = await openLockedDialog(user);
  route(SAVE, () => failure(403, "forbidden", "wrong password"));
  await enterPassword(user, dialog, "nope");
  expect(await within(dialog).findByText("That password is not right.")).toBeVisible();
  expect(screen.getByRole("dialog", { name: "Change a locked rule" })).toBeVisible();
  expect(screen.getByText("Unsaved changes")).toBeVisible();
});

test("a_lockout_shows_the_retry_sentence", async () => {
  const user = userEvent.setup();
  const dialog = await openLockedDialog(user);
  route(SAVE, () => failure(429, "too_many_requests", "slow down", { "Retry-After": "30" }));
  await enterPassword(user, dialog, "nope");
  expect(await within(dialog).findByText("Too many attempts. Try again in 30 s.")).toBeVisible();
});

test("a_stale_preview_on_save_is_explained_and_can_be_checked_again", async () => {
  const user = userEvent.setup();
  const dialog = await openLockedDialog(user);
  route(SAVE, () => failure(409, "conflict", "the review is out of date"));
  await enterPassword(user, dialog, "correct horse");
  expect(await within(dialog).findByText("The review is out of date.")).toBeVisible();
  expect(made(PREVIEW)).toHaveLength(1);

  await user.click(within(dialog).getByRole("button", { name: "Check again" }));
  await waitFor(() => expect(made(PREVIEW)).toHaveLength(2));
  expect(await screen.findByRole("dialog", { name: "Change a locked rule" })).toBeVisible();
  expect(screen.queryByText("The review is out of date.")).toBeNull();
});

test("a_draft_changed_elsewhere_blocks_the_save_and_offers_a_reload_that_asks_first", async () => {
  const user = userEvent.setup();
  const { client } = setup();
  await changeNote(user, "ask-containers", "Ask first");

  hub.draft = { ...DRAFT_4, revision: 5, updated_by: "other" };
  await act(() => client.invalidateQueries({ queryKey: ["policy"] }));
  expect(await screen.findByText("This draft changed elsewhere")).toBeVisible();
  expect(screen.getByRole("button", { name: "Save draft" })).toHaveAttribute(
    "aria-disabled",
    "true",
  );
  await user.click(screen.getByRole("button", { name: "Save draft" }));
  expect(made(PREVIEW)).toHaveLength(0);

  await user.click(screen.getByRole("button", { name: "Reload the draft" }));
  const dialog = await screen.findByRole("dialog", { name: "Discard your edits and reload?" });
  await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
  expect(screen.getByText("Unsaved changes")).toBeVisible();
  expect(screen.getByText("Revision 4")).toBeVisible();

  await user.click(screen.getByRole("button", { name: "Reload the draft" }));
  await user.click(await screen.findByRole("button", { name: "Discard edits and reload" }));
  expect(await screen.findByText("Revision 5")).toBeVisible();
  expect(screen.queryByText("Unsaved changes")).toBeNull();
  expect(screen.queryByText("This draft changed elsewhere")).toBeNull();
});

test("a_save_the_hub_rejects_shows_its_problems_in_the_banner", async () => {
  const user = userEvent.setup();
  setup();
  route(SAVE, () =>
    failure(400, "invalid", "invalid rules", undefined, [
      { path: "rules.ask-containers", message: "Unknown category.", severity: "error" },
      { path: "rules", message: "Too many rules.", severity: "error" },
    ]),
  );
  await changeNote(user, "ask-containers", "Ask first");
  await user.click(screen.getByRole("button", { name: "Save draft" }));
  expect(await screen.findByText("The hub did not accept these rules")).toBeVisible();
  expect(screen.getByText("Too many rules.")).toBeVisible();
  expect(screen.getAllByText(/Unknown category\./).length).toBeGreaterThan(0);
  expect(screen.getByText("1 unsaved change")).toBeVisible();
});

test("a_save_result_keeps_the_page_in_step_with_the_response", async () => {
  const user = userEvent.setup();
  setup();
  route(SAVE, () => {
    const saved: PolicyDraftSaved = {
      draft: { ...DRAFT_4, revision: 9 },
      locked_changes: [],
    };
    hub.draft = saved.draft;
    return saved;
  });
  await changeNote(user, "ask-containers", "Ask first");
  await user.click(screen.getByRole("button", { name: "Save draft" }));
  expect(await screen.findByText("Revision 9")).toBeVisible();
  expect(screen.queryByText("Unsaved changes")).toBeNull();
});

test("saving_a_locked_change_submits_exactly_the_rules_that_were_reviewed", async () => {
  const user = userEvent.setup();
  const dialog = await openLockedDialog(user);
  const reviewed = made(PREVIEW)[0]?.init.body as { rules: PolicyRule[] };
  expect(reviewed.rules.find((rule) => rule.id === "ask-containers")?.note).toBe("Ask first");

  await enterPassword(user, dialog, "correct horse");
  await screen.findByText(/Draft saved/);
  const body = made(SAVE)[0]?.init.body as SaveBody;
  expect(body.rules).toEqual(reviewed.rules);
  expect(body.base_revision).toBe(4);
  expect(body.preview_hash).toBe(PREVIEW_BLOCKING.preview_hash);
});

test("a_bad_request_without_rule_issues_keeps_the_dialog_open_with_a_message", async () => {
  const user = userEvent.setup();
  const dialog = await openLockedDialog(user);
  route(SAVE, () => failure(400, "invalid", "the request was malformed"));
  await enterPassword(user, dialog, "correct horse");
  await waitFor(() => expect(made(SAVE)).toHaveLength(1));
  const open = await screen.findByRole("dialog", { name: "Change a locked rule" });
  expect(within(open).getByRole("alert")).toBeVisible();
  expect(screen.queryByText("The hub did not accept these rules")).toBeNull();
});
