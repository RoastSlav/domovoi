import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Host, PolicyOverview } from "../../api/types";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import {
  DRAFT_4,
  OVERVIEW_ACTIVE,
  OVERVIEW_BERLIN,
  OVERVIEW_FRESH,
  SHADOW_PAGE_ONE,
  VERSION_3,
} from "../../test/policyFixtures";
import { renderAt } from "../../test/render";
import { PolicyPage } from "./PolicyPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const OVERVIEW = "GET /api/v1/policy";
const VERSION = "GET /api/v1/policy/versions/{number}";
const VERSIONS = "GET /api/v1/policy/versions";
const SHADOW = "GET /api/v1/policy/shadow";
const HOSTS = "GET /api/v1/hosts";
const PUT_DRAFT = "PUT /api/v1/policy/draft";
const HOST_ID = "0192a000-0000-7000-8000-000000000001";

const NAS: Host = {
  id: HOST_ID,
  name: "nas",
  kind: "agent",
  connected: true,
  agent_enrolled: true,
  open_incidents: 0,
  outdated: false,
  protocol_state: "compatible",
  reported_protocol: "1.1",
  hub_protocol: "1.1",
  reported_release: "0.0.1",
  incompatible_since: null,
};

beforeEach(() => {
  resetApi();
  localStorage.clear();
  route(OVERVIEW, () => OVERVIEW_ACTIVE);
  route(VERSION, () => VERSION_3);
  route(HOSTS, () => [NAS]);
  route(SHADOW, () => ({ ...SHADOW_PAGE_ONE, next_cursor: null }));
  route(VERSIONS, () => ({ items: [VERSION_3.summary], next_cursor: null }));
});

function overviewWith(patch: Partial<PolicyOverview>): PolicyOverview {
  return { ...OVERVIEW_ACTIVE, ...patch };
}

/** The tile holding `label`, so its number can be read with it. */
function tile(label: string): HTMLElement {
  return screen.getByText(label).parentElement as HTMLElement;
}

function show(url = "/policy") {
  return renderAt(url, "/policy", <PolicyPage />);
}

test("overview_shows_notes_shadow_status_active_version_and_rules", async () => {
  show();
  expect(await screen.findByRole("heading", { level: 1, name: "Policy" })).toBeVisible();
  for (const note of OVERVIEW_ACTIVE.notes) {
    expect(await screen.findByText(note)).toBeVisible();
  }
  const day = OVERVIEW_ACTIVE.shadow.last_24h;
  expect(tile("Recorded")).toHaveTextContent(String(day.total));
  expect(tile("Degraded")).toHaveTextContent(String(day.degraded));
  expect(tile("Differing")).toHaveTextContent(String(day.differing));
  expect(tile("Could not evaluate")).toHaveTextContent(String(day.by_status.unevaluable));
  expect(screen.getByText("Version 3")).toBeVisible();
  expect(screen.getByText(/12 rules, 1 locked, 2 shadow/)).toBeVisible();
  expect(await screen.findByText("Locked Deny")).toBeVisible();
  expect(screen.getByText("Shadow")).toBeVisible();
  expect(screen.getByText("Disabled")).toBeVisible();
  expect(screen.getAllByRole("heading", { level: 1 })).toHaveLength(1);
});

test("overview_without_a_policy_says_defaults_and_offers_a_draft", async () => {
  const user = userEvent.setup();
  route(OVERVIEW, () => OVERVIEW_FRESH);
  route(PUT_DRAFT, () => ({ draft: DRAFT_4, locked_changes: [] }));
  show();
  expect(await screen.findByText(/No policy is published yet\./)).toBeVisible();
  expect(screen.getByText(/everything else asks you/)).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Start a draft" }));
  await waitFor(() => expect(calls.some((c) => c.route === PUT_DRAFT)).toBe(true));
  expect(calls.some((c) => c.route === VERSION)).toBe(false);
});

test("overview_with_recording_off_and_no_rows_explains_why_on_the_decisions_tab", async () => {
  route(OVERVIEW, () => OVERVIEW_FRESH);
  route(SHADOW, () => ({ items: [], next_cursor: null }));
  show("/policy?tab=decisions");
  expect(await screen.findByText("Decisions are not being recorded")).toBeVisible();
  expect(screen.getAllByText(/under \[policy\] in hub\.toml/)).toHaveLength(2);
});

test("recording_on_with_no_rows_says_none_have_been_recorded", async () => {
  route(SHADOW, () => ({ items: [], next_cursor: null }));
  show("/policy?tab=decisions");
  expect(await screen.findByText(/No decisions yet\. One is recorded/)).toBeVisible();
});

test("a_last_recording_error_is_shown_as_a_danger_message_with_the_hubs_text", async () => {
  route(OVERVIEW, () => OVERVIEW_BERLIN);
  show();
  expect(await screen.findByText("The last recording pass failed")).toBeVisible();
  expect(screen.getByText("stored rules unreadable")).toBeVisible();
});

test("a_stale_draft_warns_that_the_active_version_changed", async () => {
  route(OVERVIEW, () => OVERVIEW_BERLIN);
  show();
  expect(
    await screen.findByText("The active version changed since this draft began"),
  ).toBeVisible();
  expect(screen.getByText(/Saving it again re-bases it/)).toBeVisible();
  expect(screen.getByRole("link", { name: "Edit draft" })).toHaveAttribute("href", "/policy/draft");
});

test("a_draft_based_on_the_active_version_is_not_stale", async () => {
  show();
  expect(await screen.findByText(/Draft revision 4/)).toBeVisible();
  expect(screen.queryByText("The active version changed since this draft began")).toBeNull();
});

test("rules_load_failure_is_a_load_error_inside_the_tab_only", async () => {
  route(VERSION, () => failure(500, "internal", "database is locked"));
  show();
  expect(await screen.findByText(/Could not load the rules/)).toBeVisible();
  expect(screen.getByText("Version 3")).toBeVisible();
  expect(screen.getByRole("tab", { name: "Versions" })).toBeVisible();
});

test("an_overview_failure_is_a_load_error_not_an_empty_state", async () => {
  route(OVERVIEW, () => failure(500, "internal", "database is locked"));
  show();
  expect(await screen.findByText(/Could not load the policy/)).toBeVisible();
  expect(screen.queryByText(/No policy is published yet/)).toBeNull();
});

test("versions_tab_lists_versions_with_the_active_tag_and_loads_more", async () => {
  const user = userEvent.setup();
  const older = { ...VERSION_3.summary, number: 2, rule_count: 5 };
  route(VERSIONS, (init) =>
    init.params?.query?.cursor
      ? { items: [older], next_cursor: null }
      : { items: [VERSION_3.summary], next_cursor: "c2" },
  );
  show("/policy?tab=versions");
  const link = await screen.findByRole("link", { name: "Version 3" });
  expect(link).toHaveAttribute("href", "/policy/versions/3");
  expect(within(link.closest("tr") as HTMLElement).getByText("Active")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Load more" }));
  expect(await screen.findByRole("link", { name: "Version 2" })).toBeVisible();
  expect(screen.queryByRole("button", { name: "Load more" })).toBeNull();
});

test("decisions_tab_lists_recorded_decisions_with_pills_and_flags_and_links_each_row", async () => {
  show("/policy?tab=decisions");
  const first = SHADOW_PAGE_ONE.items[0];
  await screen.findAllByText("skill.restart_plex");
  expect(screen.getAllByText("Not enforced")).toHaveLength(1);
  const hrefs = screen.getAllByRole("link").map((a) => a.getAttribute("href"));
  expect(hrefs).toContain(`/policy/decisions/${first?.execution_id}`);
  expect(screen.getAllByText("Ask for approval").length).toBeGreaterThan(0);
  expect(screen.getAllByText("Allow automatically").length).toBeGreaterThan(0);
  const table = screen.getByRole("table", { name: "Recorded decisions" });
  expect(within(table).getByText("Degraded")).toBeVisible();
  expect(within(table).getByText("Evaluated late")).toBeVisible();
  expect(screen.getAllByText("version 3").length).toBeGreaterThan(0);
  expect(screen.getAllByText("nas").length).toBeGreaterThan(0);
});

test("decisions_filters_are_sent_to_the_hub_and_follow_the_host_selector", async () => {
  const user = userEvent.setup();
  show("/policy?tab=decisions&host=nas");
  await screen.findAllByText("skill.restart_plex");
  const queries = () =>
    calls.filter((c) => c.route === SHADOW).map((c) => c.init.params?.query ?? {});
  expect(queries().at(-1)).toMatchObject({ host: HOST_ID });
  expect(queries().some((q) => q.host === undefined)).toBe(false);
  await user.click(screen.getByRole("radio", { name: "Where the two differ" }));
  await waitFor(() => expect(queries().at(-1)).toMatchObject({ host: HOST_ID, differing: true }));
  await user.click(screen.getByRole("radio", { name: "Could not evaluate" }));
  await waitFor(() =>
    expect(queries().at(-1)).toMatchObject({ host: HOST_ID, status: "unevaluable" }),
  );
});

test("a_decisions_failure_is_a_load_error", async () => {
  route(SHADOW, () => failure(500, "internal", "boom"));
  show("/policy?tab=decisions");
  expect(await screen.findByText(/Could not load the decisions/)).toBeVisible();
});

test("an_overview_without_a_draft_offers_to_start_one", async () => {
  route(OVERVIEW, () => overviewWith({ draft: null }));
  show();
  expect(await screen.findByRole("button", { name: "Start a draft" })).toBeVisible();
  expect(screen.queryByText(/Draft revision/)).toBeNull();
});

test("start_draft_copies_the_active_version_and_opens_the_editor", async () => {
  const user = userEvent.setup();
  route(OVERVIEW, () => overviewWith({ draft: null }));
  route(PUT_DRAFT, () => ({ draft: DRAFT_4, locked_changes: [] }));
  show();
  await user.click(await screen.findByRole("button", { name: "Start a draft" }));
  expect(await screen.findByText("Draft started.")).toBeVisible();
  const put = calls.find((c) => c.route === PUT_DRAFT);
  expect(put?.init.body).toEqual({ base_revision: null, rules: VERSION_3.rules });
  expect(calls.find((c) => c.route === VERSION)?.init.params?.path).toEqual({ number: 3 });
  expect(await screen.findByTestId("location")).toHaveTextContent("/policy/draft");
});
