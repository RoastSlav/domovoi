import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { failure, route } from "../../test/apiMock";
import {
  DISCARD,
  PREVIEW,
  SAVE,
  changeNote,
  editRule,
  hub,
  made,
  setup,
} from "../../test/draftPage";
import {
  DRAFT_4,
  OVERVIEW_ACTIVE,
  OVERVIEW_BERLIN,
  PREVIEW_BLOCKING,
  PREVIEW_INVALID,
} from "../../test/policyFixtures";

vi.mock("../../api/client", () => import("../../test/apiMock"));

// Every test types and clicks through menus; the default 5 s is tight on a busy machine.
vi.setConfig({ testTimeout: 30_000 });

const here = () => screen.getByTestId("location").textContent;

test("editor_lists_the_draft_rules_with_locked_shadow_and_disabled_marks", async () => {
  setup();
  const table = await screen.findByRole("table", { name: "Rules" });
  for (const id of ["no-destroy", "plex-restart", "ask-containers", "old-ask", "no-prune"]) {
    expect(within(table).getByText(id)).toBeVisible();
  }
  expect(within(table).getAllByText("Locked Deny")).toHaveLength(2);
  expect(within(table).getByText("Shadow")).toBeVisible();
  expect(within(table).getByText("Disabled")).toBeVisible();
  expect(screen.getByText("Revision 4")).toBeVisible();
  expect(screen.getByText("Based on version 3")).toBeVisible();
});

test("editing_a_rule_changes_the_working_copy_and_shows_unsaved_changes_without_saving", async () => {
  const user = userEvent.setup();
  setup();
  await changeNote(user, "ask-containers", "Ask before any restart");
  expect(await screen.findByText("Ask before any restart")).toBeVisible();
  expect(screen.getByText("Unsaved changes")).toBeVisible();
  expect(screen.getByText("1 unsaved change")).toBeVisible();
  expect(here()).toBe("/policy/draft");
  expect(made(SAVE)).toHaveLength(0);
});

test("adding_a_rule_starts_it_as_a_shadow_rule", async () => {
  const user = userEvent.setup();
  setup();
  await user.click(await screen.findByRole("link", { name: "Add rule" }));
  expect(here()).toBe("/policy/draft?rule=new");
  expect(screen.getByRole("switch", { name: /^Shadow/ })).toBeChecked();
  await user.click(screen.getByRole("button", { name: "Apply to draft" }));
  const row = (await screen.findByText("new-rule")).closest("tr") as HTMLElement;
  expect(within(row).getByText("Shadow")).toBeVisible();
  expect(screen.getByText("Unsaved changes")).toBeVisible();
});

test("locked_switch_only_exists_for_a_deny_and_is_cleared_when_the_outcome_changes", async () => {
  const user = userEvent.setup();
  setup();
  await editRule(user, "no-prune");
  const locked = () => screen.queryByRole("switch", { name: /^Locked Deny/ });
  expect(locked()).toBeChecked();
  await user.click(screen.getByRole("radio", { name: "Allow automatically" }));
  expect(locked()).toBeNull();
  await user.click(screen.getByRole("radio", { name: "Deny" }));
  expect(locked()).not.toBeChecked();
  expect(screen.getByText(/asks for your password and shows what it would/)).toBeVisible();
});

test("invalid_rule_fields_show_field_errors_and_do_not_apply", async () => {
  const user = userEvent.setup();
  setup();
  await user.click(await screen.findByRole("link", { name: "Add rule" }));
  await user.clear(screen.getByRole("textbox", { name: "Rule name" }));
  await user.click(screen.getByRole("button", { name: "Add condition" }));
  await user.click(screen.getByRole("button", { name: "Apply to draft" }));
  expect(await screen.findByText("Give the rule a name.")).toBeVisible();
  expect(screen.getByText("Enter a whole number.")).toBeVisible();
  expect(here()).toBe("/policy/draft?rule=new");
  expect(screen.queryByText("Unsaved changes")).toBeNull();
});

test("removing_and_duplicating_rules_work_and_a_locked_removal_warns_about_the_password", async () => {
  const user = userEvent.setup();
  setup();
  await user.click(await screen.findByRole("button", { name: "Actions for ask-containers" }));
  await user.click(screen.getByRole("menuitem", { name: "Duplicate" }));
  expect(
    await screen.findByRole("heading", { name: "Edit rule ask-containers-copy" }),
  ).toBeVisible();
  expect(here()).toBe("/policy/draft?rule=ask-containers-copy");
  await user.click(screen.getByRole("button", { name: "Back to the rules" }));

  await user.click(screen.getByRole("button", { name: "Actions for old-ask" }));
  await user.click(screen.getByRole("menuitem", { name: "Remove" }));
  expect(screen.queryByText("old-ask")).toBeNull();

  await user.click(screen.getByRole("button", { name: "Actions for no-prune" }));
  await user.click(screen.getByRole("menuitem", { name: "Remove" }));
  const dialog = await screen.findByRole("dialog", { name: "Remove no-prune?" });
  expect(within(dialog).getByText(/asks for your password/)).toBeVisible();
  await user.click(within(dialog).getByRole("button", { name: "Remove rule" }));
  expect(screen.queryByText("no-prune")).toBeNull();
  expect(screen.getByText("3 unsaved changes")).toBeVisible();
});

test("a_rule_with_an_unknown_outcome_opens_read_only_and_cannot_be_applied", async () => {
  const user = userEvent.setup();
  const rules = DRAFT_4.rules as unknown[];
  rules.push({ ...DRAFT_4.rules[0], id: "from-newer", outcome: "from_the_future" });
  try {
    setup();
    await editRule(user, "from-newer");
    expect(
      await screen.findByText("This rule uses something this page does not know"),
    ).toBeVisible();
    expect(screen.getByText(/It is kept as it is/)).toBeVisible();
    expect(screen.queryByRole("radio", { name: "Deny" })).toBeNull();
    expect(screen.queryByRole("textbox", { name: "Note" })).toBeNull();
    const apply = screen.getByRole("button", { name: "Apply to draft" });
    expect(apply).toHaveAttribute("aria-disabled", "true");
    await user.click(apply);
    expect(screen.queryByText("Unsaved changes")).toBeNull();
    await user.click(screen.getByRole("button", { name: "Back to the rules" }));
    expect(
      within(await screen.findByRole("table", { name: "Rules" })).getByText("from-newer"),
    ).toBeVisible();
  } finally {
    rules.pop();
  }
});

test("an_unknown_rule_in_the_address_is_dropped", async () => {
  setup(OVERVIEW_ACTIVE, "/policy/draft?rule=nope");
  await screen.findByRole("table", { name: "Rules" });
  await waitFor(() => expect(here()).toBe("/policy/draft"));
});

test("check_rules_shows_validation_conflicts_locks_and_the_simulation_and_changes_nothing", async () => {
  const user = userEvent.setup();
  setup();
  hub.preview = PREVIEW_INVALID;
  await user.click(await screen.findByRole("button", { name: "Check rules" }));
  expect(
    await screen.findByText("The time window starts and ends at the same time."),
  ).toBeVisible();
  expect(screen.getByRole("link", { name: "plex-restart" })).toHaveAttribute(
    "href",
    "/policy/draft?rule=plex-restart",
  );
  expect(screen.getByText("Whole draft")).toBeVisible();
  expect(
    screen.getByText("Checking changes nothing. Saving and publishing are separate steps."),
  ).toBeVisible();

  hub.preview = PREVIEW_BLOCKING;
  await user.click(screen.getByRole("button", { name: "Check rules" }));
  expect(await screen.findByText("Conflicting overlap")).toBeVisible();
  expect(screen.getByText("2 findings")).toBeVisible();
  expect(screen.getByText("Overridden by a locked rule")).toBeVisible();
  expect(screen.getByText("Locked rule added")).toBeVisible();
  expect(screen.getByText("Locked rules changed: review them before saving.")).toBeVisible();
  expect(screen.getByText("Would become Allow automatically")).toBeVisible();
  expect(screen.queryByText("The time window starts and ends at the same time.")).toBeNull();
  const body = made(PREVIEW)[0]?.init.body as { base_revision: number; rules: unknown[] };
  expect(body.base_revision).toBe(4);
  expect(body.rules).toHaveLength(6);
  expect(made(SAVE)).toHaveLength(0);
});

test("a_failed_check_names_the_rules_the_hub_rejected", async () => {
  const user = userEvent.setup();
  setup();
  route(PREVIEW, () =>
    failure(400, "invalid", "invalid rules", undefined, [
      { path: "rules.plex-restart", message: "Unknown capability.", severity: "error" },
    ]),
  );
  await user.click(await screen.findByRole("button", { name: "Check rules" }));
  expect(await screen.findByText("The hub did not accept these rules")).toBeVisible();
  expect(screen.getByText(/Unknown capability\./)).toBeVisible();
});

test("the_time_window_zone_is_whatever_the_hub_reports", async () => {
  const user = userEvent.setup();
  setup(OVERVIEW_BERLIN);
  await editRule(user, "plex-restart");
  expect(await screen.findByLabelText("Start (Europe/Berlin)")).toHaveValue("23:00");
  expect(screen.getByLabelText("End (Europe/Berlin)")).toBeVisible();
  expect(screen.getAllByText(/Times are read in Europe\/Berlin/).length).toBeGreaterThan(0);
  expect(screen.queryByText(/UTC/)).toBeNull();
});

test("the_zone_comes_from_the_hub_even_when_it_is_utc", async () => {
  const user = userEvent.setup();
  setup(OVERVIEW_ACTIVE);
  await editRule(user, "plex-restart");
  expect(await screen.findByLabelText("Start (UTC)")).toBeVisible();
});

test("a_missing_zone_disables_the_time_inputs_with_a_reason", async () => {
  const user = userEvent.setup();
  setup({ ...OVERVIEW_ACTIVE, time_window_zone: "" });
  await editRule(user, "plex-restart");
  const start = await screen.findByLabelText(/^Start/);
  expect(start).toHaveAttribute("readonly");
  expect(screen.getAllByText("The hub did not say which time zone it uses.")).toHaveLength(2);
});

test("leaving_the_editor_with_unsaved_edits_asks_first_and_says_draft", async () => {
  const user = userEvent.setup();
  setup();
  await changeNote(user, "ask-containers", "Ask first");
  await user.click(screen.getByRole("link", { name: "Leave the page" }));
  const dialog = await screen.findByRole("dialog", { name: "Leave without saving?" });
  expect(within(dialog).getByText(/changes to this draft are lost/)).toBeVisible();
  await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
  expect(here()).toBe("/policy/draft");
  await user.click(screen.getByRole("link", { name: "Leave the page" }));
  await user.click(await screen.findByRole("button", { name: "Leave without saving" }));
  expect(await screen.findByText("other page")).toBeVisible();
});

test("discarding_the_draft_asks_first_and_sends_the_revision", async () => {
  const user = userEvent.setup();
  setup();
  await user.click(await screen.findByRole("button", { name: "More actions" }));
  await user.click(screen.getByRole("menuitem", { name: "Discard draft..." }));
  const dialog = await screen.findByRole("dialog", { name: "Discard the draft?" });
  await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
  expect(made(DISCARD)).toHaveLength(0);

  await user.click(screen.getByRole("button", { name: "More actions" }));
  await user.click(screen.getByRole("menuitem", { name: "Discard draft..." }));
  await user.click(await screen.findByRole("button", { name: "Discard draft" }));
  await waitFor(() => expect(made(DISCARD)).toHaveLength(1));
  expect(made(DISCARD)[0]?.init.params?.query).toEqual({ revision: 4 });
  await waitFor(() => expect(here()).toBe("/policy"));
});

test("save_is_the_primary_action_while_dirty_and_review_and_publish_otherwise", async () => {
  const user = userEvent.setup();
  setup();
  const publish = await screen.findByRole("link", { name: "Review and publish" });
  expect(publish).toHaveAttribute("href", "/policy/draft/publish");
  expect(screen.queryByRole("button", { name: "Save draft" })).toBeNull();

  await changeNote(user, "ask-containers", "Changed");
  expect(screen.queryByRole("link", { name: "Review and publish" })).toBeNull();
  expect(screen.getByRole("button", { name: "Review and publish" })).toHaveAttribute(
    "aria-disabled",
    "true",
  );
  expect(screen.getByRole("button", { name: "Save draft" })).toBeVisible();
});

test("a_draft_that_is_not_based_on_the_active_version_says_so", async () => {
  setup(OVERVIEW_BERLIN);
  expect(
    await screen.findByText("The active version changed since this draft began"),
  ).toBeVisible();
  expect(screen.getByRole("link", { name: "Simulate the draft" })).toHaveAttribute(
    "href",
    "/policy/simulator?rules=draft",
  );
});
