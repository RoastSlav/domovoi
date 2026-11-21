import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { DRAFT_4, OVERVIEW_ACTIVE, VERSION_3 } from "../../test/policyFixtures";
import { renderAt } from "../../test/render";
import { VersionPage } from "./VersionPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const OVERVIEW = "GET /api/v1/policy";
const VERSION = "GET /api/v1/policy/versions/{number}";
const PUT_DRAFT = "PUT /api/v1/policy/draft";

beforeEach(() => {
  resetApi();
  localStorage.clear();
  route(OVERVIEW, () => ({ ...OVERVIEW_ACTIVE, draft: null }));
  route(VERSION, () => VERSION_3);
  route("GET /api/v1/hosts", () => []);
});

function show(number: string) {
  return renderAt(`/policy/versions/${number}`, "/policy/versions/:number", <VersionPage />);
}

test("version_page_shows_rules_acknowledgements_and_the_active_state", async () => {
  show("3");
  expect(await screen.findByRole("heading", { level: 1, name: "Version 3" })).toBeVisible();
  expect(await screen.findByText("Active")).toBeVisible();
  expect(await screen.findByText("Locked Deny")).toBeVisible();
  const ack = VERSION_3.acknowledgements[0];
  const acks = screen.getByRole("table", { name: "Acknowledged conflicts" });
  expect(within(acks).getByText("Conflicting overlap")).toBeVisible();
  expect(within(acks).getByText(ack?.first_rule ?? "")).toBeVisible();
  expect(within(acks).getByText(ack?.second_rule ?? "")).toBeVisible();
  expect(screen.getByRole("link", { name: "Simulate" })).toHaveAttribute(
    "href",
    "/policy/simulator?rules=version-3",
  );
});

test("an_older_version_says_which_version_replaced_it", async () => {
  route(VERSION, () => ({
    ...VERSION_3,
    summary: { ...VERSION_3.summary, number: 2 },
    acknowledgements: [],
  }));
  show("2");
  expect(await screen.findByText("Replaced by version 3")).toBeVisible();
  expect(
    await screen.findByText("No conflicts needed acknowledging for this version."),
  ).toBeVisible();
});

test("no_such_version_says_so_with_a_link_back", async () => {
  route(VERSION, () => failure(404, "not_found", "no such version"));
  show("9");
  expect(await screen.findByText("No such version")).toBeVisible();
  expect(screen.getByRole("link", { name: "Back to Policy" })).toHaveAttribute("href", "/policy");
});

test("a_version_that_is_not_a_number_is_no_such_version", async () => {
  show("abc");
  expect(await screen.findByText("No such version")).toBeVisible();
  expect(calls.some((c) => c.route === VERSION)).toBe(false);
});

test("other_failures_are_a_load_error", async () => {
  route(VERSION, () => failure(500, "internal", "boom"));
  show("3");
  expect(await screen.findByText(/Could not load the version/)).toBeVisible();
});

test("a_version_cannot_start_a_draft_while_one_exists", async () => {
  route(OVERVIEW, () => OVERVIEW_ACTIVE);
  show("3");
  const button = await screen.findByRole("button", { name: "Start a draft from this version" });
  await screen.findByText("Active");
  expect(button).toHaveAttribute("aria-disabled", "true");
});

test("a_version_starts_a_draft_from_its_own_rules", async () => {
  const user = userEvent.setup();
  route(PUT_DRAFT, () => ({ draft: DRAFT_4, locked_changes: [] }));
  show("3");
  await user.click(await screen.findByRole("button", { name: "Start a draft from this version" }));
  expect(await screen.findByText("Draft started.")).toBeVisible();
  expect(calls.find((c) => c.route === PUT_DRAFT)?.init.body).toEqual({
    base_revision: null,
    rules: VERSION_3.rules,
  });
});
