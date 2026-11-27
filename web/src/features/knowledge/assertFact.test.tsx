import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { failure, resetApi, route } from "../../test/apiMock";
import { knowledgeFact } from "../../test/knowledgeFixtures";
import { callsTo, lastBody, mockKnowledgeApi } from "../../test/knowledgeRoutes";
import { renderAt } from "../../test/render";
import { KnowledgePage } from "./KnowledgePage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const ASSERT = "POST /api/v1/knowledge/facts";

beforeEach(() => {
  resetApi();
  mockKnowledgeApi();
  route(ASSERT, () => knowledgeFact({ id: "new" }));
});

async function openDialog() {
  const user = userEvent.setup();
  renderAt("/knowledge", "/knowledge", <KnowledgePage />);
  await screen.findByRole("table", { name: "Facts" });
  await user.click(screen.getByRole("button", { name: "Add a fact" }));
  const dialog = await screen.findByRole("dialog", { name: "Add a fact" });
  return { user, dialog };
}

async function pickNas(dialog: HTMLElement, user: ReturnType<typeof userEvent.setup>) {
  await within(dialog).findByRole("option", { name: "nas" });
  await user.selectOptions(within(dialog).getByRole("combobox", { name: "Host" }), "host-nas");
}

test("adding_a_fact_offers_only_assertable_predicates_and_matching_subject_kinds", async () => {
  const { user, dialog } = await openDialog();
  const kind = within(dialog).getByRole("combobox", { name: "Kind" });

  const labels = within(kind)
    .getAllByRole("option")
    .map((option) => option.textContent);
  expect(labels).toContain("Management path");
  expect(labels).not.toContain("Platform");

  await user.selectOptions(kind, "management_path");
  expect(within(dialog).queryByRole("combobox", { name: "About is" })).toBeNull();
  expect(within(dialog).getByRole("combobox", { name: "Host" })).toBeVisible();

  await user.selectOptions(kind, "never_restart");
  const about = within(dialog).getByRole("combobox", { name: "About is" });
  expect(
    within(about)
      .getAllByRole("option")
      .map((option) => option.textContent),
  ).toEqual(["A container", "A service"]);
});

test("adding_a_fact_sends_subject_predicate_and_value_and_never_source_or_actor", async () => {
  const { user, dialog } = await openDialog();

  await user.click(within(dialog).getByRole("button", { name: "Save fact" }));
  expect(await within(dialog).findByText("Choose what it is about.")).toBeVisible();
  expect(within(dialog).getByText("Write 1 to 80 characters.")).toBeVisible();
  expect(callsTo(ASSERT)).toHaveLength(0);

  await pickNas(dialog, user);
  await user.type(within(dialog).getByRole("textbox", { name: "Role" }), "  media server ");
  await user.click(within(dialog).getByRole("button", { name: "Save fact" }));

  expect(await screen.findByText("Fact saved.")).toBeVisible();
  const body = lastBody(ASSERT);
  expect(body).toEqual({ subject: "host:host-nas", predicate: "role", value: "media server" });
  expect(body).not.toHaveProperty("password");
  expect(body).not.toHaveProperty("source");
  expect(body).not.toHaveProperty("actor");
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
});

test("adding_a_safety_critical_fact_swaps_the_form_for_the_password_prompt_and_cancel_returns_to_it", async () => {
  const { user, dialog } = await openDialog();
  await user.selectOptions(
    within(dialog).getByRole("combobox", { name: "Kind" }),
    "management_path",
  );
  await pickNas(dialog, user);
  await user.type(within(dialog).getByRole("textbox", { name: "Network" }), "192.168.1.7/24");

  await user.click(within(dialog).getByRole("button", { name: "Save fact" }));
  const prompt = await screen.findByRole("dialog", { name: "Confirm with your password" });
  expect(screen.queryByRole("dialog", { name: "Add a fact" })).toBeNull();
  expect(callsTo(ASSERT)).toHaveLength(0);

  await user.click(within(prompt).getByRole("button", { name: "Cancel" }));
  const back = await screen.findByRole("dialog", { name: "Add a fact" });
  expect(within(back).getByRole("textbox", { name: "Network" })).toHaveValue("192.168.1.7/24");

  await user.click(within(back).getByRole("button", { name: "Save fact" }));
  const again = await screen.findByRole("dialog", { name: "Confirm with your password" });
  await user.type(within(again).getByLabelText("Password"), "correct horse");
  await user.click(within(again).getByRole("button", { name: "Save" }));

  await waitFor(() => expect(callsTo(ASSERT)).toHaveLength(1));
  expect(lastBody(ASSERT)).toEqual({
    subject: "host:host-nas",
    predicate: "management_path",
    value: { cidr: "192.168.1.7/24" },
    password: "correct horse",
  });
});

// The page checks the form of a network itself; what only the hub refuses still reads here.
test("the_hubs_400_for_a_bad_cidr_shows_inside_the_dialog", async () => {
  route(ASSERT, () => failure(400, "bad_request", "that is not a network in CIDR form"));
  const { user, dialog } = await openDialog();
  await user.selectOptions(within(dialog).getByRole("combobox", { name: "Kind" }), "trusted_cidr");
  await pickNas(dialog, user);
  await user.type(within(dialog).getByRole("textbox", { name: "Network" }), "10.0.0.0/8");
  await user.click(within(dialog).getByRole("button", { name: "Save fact" }));

  const prompt = await screen.findByRole("dialog", { name: "Confirm with your password" });
  await user.type(within(prompt).getByLabelText("Password"), "correct horse");
  await user.click(within(prompt).getByRole("button", { name: "Save" }));

  expect(await within(prompt).findByText(/not a network in CIDR form/i)).toBeVisible();
});

test("a_wrong_password_for_a_safety_critical_fact_keeps_the_prompt_open", async () => {
  route(ASSERT, () => failure(403, "forbidden", "this change needs your password"));
  const { user, dialog } = await openDialog();
  await user.selectOptions(within(dialog).getByRole("combobox", { name: "Kind" }), "trusted_cidr");
  await pickNas(dialog, user);
  await user.type(within(dialog).getByRole("textbox", { name: "Network" }), "10.0.0.0/8");
  await user.click(within(dialog).getByRole("button", { name: "Save fact" }));

  const prompt = await screen.findByRole("dialog", { name: "Confirm with your password" });
  await user.type(within(prompt).getByLabelText("Password"), "wrong");
  await user.click(within(prompt).getByRole("button", { name: "Save" }));

  expect(await within(prompt).findByText("That password is not right.")).toBeVisible();
});
