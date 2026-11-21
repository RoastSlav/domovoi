import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { failure, resetApi, route } from "../../test/apiMock";
import {
  PATHS_AVAILABLE,
  PATHS_UNAVAILABLE,
  knowledgeFact,
  managementPath,
} from "../../test/knowledgeFixtures";
import { callsTo, lastBody, mockKnowledgeApi } from "../../test/knowledgeRoutes";
import { renderAt } from "../../test/render";
import { PathsPage } from "./PathsPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const PATHS = "GET /api/v1/knowledge/hosts/{name}/management-paths";
const CONFIRM = "POST /api/v1/knowledge/facts/{id}/confirm";
const ACCEPT = "POST /api/v1/knowledge/facts/{id}/accept";
const RETRACT = "POST /api/v1/knowledge/facts/{id}/retract";
const ASSERT = "POST /api/v1/knowledge/facts";
const FORBIDDEN_WORDS = /\b(covered|protected)\b/i;

beforeEach(() => {
  resetApi();
  mockKnowledgeApi();
  route(PATHS, () => PATHS_UNAVAILABLE);
  route(CONFIRM, () => managementPath({ state: "confirmed" }));
  route(ACCEPT, () => knowledgeFact({ id: "mine", predicate: "management_path" }));
  route(RETRACT, () => knowledgeFact({ status: "retracted" }));
  route(ASSERT, () => knowledgeFact({ id: "new", predicate: "management_path" }));
});

function show(url = "/knowledge/paths?host=nas") {
  return renderAt(url, "/knowledge/paths", <PathsPage />);
}

/** The table row that shows `cidr`. */
async function rowOf(cidr: string): Promise<HTMLElement> {
  const link = await screen.findByRole("link", { name: cidr });
  const row = link.closest("tr");
  if (!row) {
    throw new Error(`no row for ${cidr}`);
  }
  return row;
}

async function askForPassword(user: ReturnType<typeof userEvent.setup>, label: string) {
  const prompt = await screen.findByRole("dialog", { name: "Confirm with your password" });
  await user.type(within(prompt).getByLabelText("Password"), "correct horse");
  await user.click(within(prompt).getByRole("button", { name: label }));
  return prompt;
}

test("coverage_unavailable_is_shown_honestly_and_nothing_reads_covered", async () => {
  show();

  expect(await screen.findByText("Cannot be verified yet")).toBeVisible();
  expect(screen.getByText("Unattended containment is blocked on nas")).toBeVisible();
  expect(screen.getByText("The helper's coverage cannot be verified yet.")).toBeVisible();
  expect(document.body.textContent).not.toMatch(FORBIDDEN_WORDS);
});

test("confirm_is_disabled_with_its_reason_and_sends_nothing_while_coverage_is_unavailable", async () => {
  const user = userEvent.setup();
  show();
  const confirm = within(await rowOf("10.8.0.0/24")).getByRole("button", { name: "Confirm" });
  const again = within(await rowOf("172.16.0.0/16")).getByRole("button", {
    name: "Confirm again",
  });

  expect(confirm).toHaveAttribute("aria-disabled", "true");
  await user.click(confirm);
  await user.click(again);

  expect(screen.queryByRole("dialog")).toBeNull();
  expect(callsTo(CONFIRM)).toHaveLength(0);
  await user.hover(confirm);
  expect(
    await screen.findByText(/Coverage cannot be verified yet, so no path can be confirmed/),
  ).toBeVisible();
});

test("the_blocked_message_lists_every_reason_and_says_monitoring_is_unaffected", async () => {
  show();

  await screen.findByText("Unattended containment is blocked on nas");
  for (const reason of [
    "The helper's coverage cannot be verified yet.",
    "1 path found by Domovoi is not accepted by you yet.",
    "1 path is not confirmed.",
    "1 path is stale.",
  ]) {
    expect(screen.getByText(reason)).toBeVisible();
  }
  expect(
    screen.getByText("Monitoring and actions you approve yourself are not affected."),
  ).toBeVisible();
});

test("paths_show_state_source_freshness_and_confirmation", async () => {
  route(PATHS, () => ({
    ...PATHS_AVAILABLE,
    paths: [
      ...PATHS_UNAVAILABLE.paths,
      managementPath({
        state: "confirmed",
        fact: knowledgeFact({
          id: "path-done",
          predicate: "management_path",
          value: { cidr: "10.20.0.0/24" },
        }),
        confirmation: { confirmed_by: "user:me", confirmed_at: "2026-11-03T10:00:00Z" },
      }),
    ],
  }));
  show();

  const candidate = within(await rowOf("192.168.1.0/24"));
  expect(candidate.getAllByText("Not yours yet").length).toBeGreaterThan(0);
  expect(candidate.getByText("Found by Domovoi")).toBeVisible();
  expect(candidate.getByText(/Fresh until/)).toBeVisible();
  expect(candidate.getByText("Not confirmed")).toBeVisible();

  const unconfirmed = within(await rowOf("10.8.0.0/24"));
  expect(unconfirmed.getByText("Waiting for you")).toBeVisible();
  expect(unconfirmed.getByText("You")).toBeVisible();

  const stale = within(await rowOf("172.16.0.0/16"));
  expect(stale.getByText(/Stale since/)).toBeVisible();

  const done = within(await rowOf("10.20.0.0/24"));
  expect(await done.findByText(/Confirmed by You/)).toBeVisible();
});

test("a_confirmed_path_says_who_confirmed_it_and_nothing_about_coverage", async () => {
  route(PATHS, () => PATHS_AVAILABLE);
  show();

  const confirmed = within(await rowOf("192.168.1.0/24"));
  expect(await confirmed.findByText(/Confirmed by You/)).toBeVisible();
  expect(confirmed.getByRole("button", { name: "Retract" })).toBeVisible();
  expect(confirmed.queryByRole("button", { name: /Confirm/ })).toBeNull();
});

test("accepting_a_discovered_path_asks_for_the_password_and_moves_it_to_unconfirmed_after_refetch", async () => {
  const user = userEvent.setup();
  let accepted = false;
  route(PATHS, () =>
    accepted
      ? {
          ...PATHS_UNAVAILABLE,
          paths: PATHS_UNAVAILABLE.paths.map((path) =>
            path.state === "candidate" ? { ...path, state: "unconfirmed" } : path,
          ),
        }
      : PATHS_UNAVAILABLE,
  );
  route(ACCEPT, () => {
    accepted = true;
    return knowledgeFact({ id: "mine", predicate: "management_path" });
  });
  show();

  await user.click(
    within(await rowOf("192.168.1.0/24")).getByRole("button", { name: "Accept as mine" }),
  );
  expect(callsTo(ACCEPT)).toHaveLength(0);
  await askForPassword(user, "Accept");

  await waitFor(() => expect(callsTo(ACCEPT)).toHaveLength(1));
  expect(lastBody(ACCEPT)).toEqual({
    expected_version: PATHS_UNAVAILABLE.paths[0]?.fact.version,
    password: "correct horse",
  });
  expect(await screen.findByText("Saved as your own path. Confirm it next.")).toBeVisible();
  await waitFor(() => expect(screen.queryByText("Not yours yet")).toBeNull());
});

test("confirming_with_coverage_available_asks_for_the_password_and_sends_version_and_password", async () => {
  const user = userEvent.setup();
  route(PATHS, () => PATHS_AVAILABLE);
  show();

  expect(await screen.findByText("Checked when you confirm")).toBeVisible();
  await user.click(within(await rowOf("10.8.0.0/24")).getByRole("button", { name: "Confirm" }));
  expect(callsTo(CONFIRM)).toHaveLength(0);
  await askForPassword(user, "Confirm");

  await waitFor(() => expect(callsTo(CONFIRM)).toHaveLength(1));
  expect(lastBody(CONFIRM)).toEqual({
    expected_version: PATHS_AVAILABLE.paths[0]?.fact.version,
    password: "correct horse",
  });
  expect(await screen.findByText("Path confirmed.")).toBeVisible();
});

test.each([
  ["the helper does not cover this path", "the helper does not cover this path"],
  ["someone changed it meanwhile; reload and try again", "someone changed it meanwhile"],
])(
  "a_confirm_conflict_shows_the_hubs_sentence_inside_the_prompt: %s",
  async (message, expected) => {
    const user = userEvent.setup();
    route(PATHS, () => PATHS_AVAILABLE);
    route(CONFIRM, () => failure(409, "conflict", message));
    show();

    await user.click(within(await rowOf("10.8.0.0/24")).getByRole("button", { name: "Confirm" }));
    const prompt = await askForPassword(user, "Confirm");

    expect(await within(prompt).findByText(new RegExp(expected, "i"))).toBeVisible();
    expect(screen.getByRole("dialog", { name: "Confirm with your password" })).toBeVisible();
  },
);

test("a_wrong_password_on_confirm_keeps_the_prompt_open_and_changes_nothing", async () => {
  const user = userEvent.setup();
  route(PATHS, () => PATHS_AVAILABLE);
  route(CONFIRM, () => failure(403, "forbidden", "this change needs your password"));
  show();

  await user.click(within(await rowOf("10.8.0.0/24")).getByRole("button", { name: "Confirm" }));
  const prompt = await askForPassword(user, "Confirm");

  expect(await within(prompt).findByText("That password is not right.")).toBeVisible();
  expect(screen.queryByText("Path confirmed.")).toBeNull();
  expect(callsTo(CONFIRM)).toHaveLength(1);
});

test("adding_a_path_uses_the_password_prompt_and_presets_predicate_and_host", async () => {
  const user = userEvent.setup();
  show();

  await user.click(await screen.findByRole("button", { name: "Add a management path" }));
  const dialog = await screen.findByRole("dialog", { name: "Add a fact" });
  expect(within(dialog).queryByRole("combobox", { name: "Kind" })).toBeNull();
  expect(within(dialog).queryByRole("combobox", { name: "Host" })).toBeNull();
  await user.type(within(dialog).getByRole("textbox", { name: "Network" }), "10.9.0.0/24");
  await user.click(within(dialog).getByRole("button", { name: "Save fact" }));
  await askForPassword(user, "Save");

  await waitFor(() => expect(callsTo(ASSERT)).toHaveLength(1));
  expect(lastBody(ASSERT)).toEqual({
    subject: "host:host-nas",
    predicate: "management_path",
    value: { cidr: "10.9.0.0/24" },
    password: "correct horse",
  });
});

test("retracting_a_path_needs_no_password", async () => {
  const user = userEvent.setup();
  show();

  await user.click(within(await rowOf("10.8.0.0/24")).getByRole("button", { name: "Retract" }));
  const dialog = await screen.findByRole("dialog", { name: "Retract this management path?" });
  expect(within(dialog).queryByLabelText("Password")).toBeNull();
  await user.click(within(dialog).getByRole("button", { name: "Retract" }));

  expect(await screen.findByText("Path retracted.")).toBeVisible();
  expect(lastBody(RETRACT)).toEqual({
    expected_version: PATHS_UNAVAILABLE.paths[1]?.fact.version,
  });
});

test("no_host_selected_shows_the_host_chooser_and_makes_no_paths_request", async () => {
  const user = userEvent.setup();
  show("/knowledge/paths");

  expect(await screen.findByRole("heading", { name: "Choose a host" })).toBeVisible();
  expect(callsTo(PATHS)).toHaveLength(0);
  await user.click(await screen.findByRole("button", { name: "Show paths of nas" }));

  await waitFor(() => expect(screen.getByTestId("location")).toHaveTextContent("host=nas"));
  expect(await screen.findByText("Unattended containment is blocked on nas")).toBeVisible();
  expect(callsTo(PATHS)).toHaveLength(1);
});

test("unknown_host_and_failed_load_are_distinct", async () => {
  route(PATHS, () => failure(404, "not_found", "no such host"));
  const missing = show();
  expect(await screen.findByText("No such host")).toBeVisible();
  missing.unmount();

  route(PATHS, () => failure(500, "internal", "boom"));
  const broken = show();
  expect(await screen.findByRole("button", { name: "Retry" })).toBeVisible();
  expect(screen.queryByText("No such host")).toBeNull();
  expect(screen.queryByText("No management paths yet")).toBeNull();
  broken.unmount();

  route(PATHS, () => ({ host: "nas", coverage_available: false, paths: [] }));
  show();
  expect(await screen.findByText("No management paths yet")).toBeVisible();
  expect(screen.getByText("No management paths are recorded for this host.")).toBeVisible();
});

test("all_confirmed_with_coverage_available_is_not_blocked_but_says_other_conditions_apply", async () => {
  route(PATHS, () => ({ ...PATHS_AVAILABLE, paths: PATHS_AVAILABLE.paths.slice(1) }));
  show();

  expect(
    await screen.findByText("Management paths are confirmed and current on nas"),
  ).toBeVisible();
  expect(
    screen.getByText(/also needs the capability, the policy and the helper to allow it/),
  ).toBeVisible();
  expect(screen.queryByText(/is blocked on nas/)).toBeNull();
  expect(document.body.textContent).not.toMatch(FORBIDDEN_WORDS);
});
