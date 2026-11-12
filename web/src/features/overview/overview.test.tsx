import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { action, incident, nas, overview } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { OverviewPage } from "./OverviewPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const pi = { ...nas, id: "host-pi", name: "pi", connected: false, open_incidents: 0 };

beforeEach(() => {
  resetApi();
  localStorage.clear();
  route("GET /api/v1/hosts", () => [nas, pi]);
  route("GET /api/v1/overview", () => overview());
  route("GET /api/v1/incidents", () => ({ items: [], next_cursor: null }));
  route("GET /api/v1/actions", () => ({ items: [], next_cursor: null }));
});

function open(url = "/") {
  return renderAt(url, "/", <OverviewPage />);
}

const tile = (name: RegExp) => screen.findByRole("link", { name });

test("tiles_show_what_needs_the_person_and_lead_to_their_lists", async () => {
  route("GET /api/v1/overview", () =>
    overview({ incidents_action_needed: 2, incidents_open: 5, approvals_waiting: 3 }),
  );
  open();

  expect(await tile(/^Needs attention 2/)).toHaveAttribute("href", "/incidents");
  expect(await tile(/^Open incidents 5/)).toHaveAttribute("href", "/incidents");
  expect(await tile(/^Waiting for approval 3/)).toHaveAttribute("href", "/approvals");
  const hosts = await tile(/^Hosts online 1 of 2/);
  expect(hosts).toHaveAttribute("href", "/hosts");
  expect(hosts, "what is down is said in words, not only by colour").toHaveTextContent("1 offline");
});

test("hosts_online_counts_connected_agents_and_says_who_never_connected", async () => {
  route("GET /api/v1/hosts", () => [
    nas,
    { ...nas, id: "h2", name: "never", connected: false, agent_enrolled: false },
    { ...nas, id: "h3", name: "hub", kind: "local", connected: false, agent_enrolled: false },
  ]);
  open();

  const hosts = await tile(/^Hosts online 1 of 2/);
  expect(hosts, "the hub's own machine is not counted").toHaveTextContent("1 never connected");
});

test("tiles_say_all_clear_when_nothing_is_wrong", async () => {
  route("GET /api/v1/hosts", () => [nas]);
  open();

  expect(await tile(/^Needs attention 0/)).toHaveTextContent("All clear");
  expect(await tile(/^Hosts online 1 of 1/)).toHaveTextContent("All connected");
});

test("overview_says_nothing_needs_you_when_both_lists_are_empty", async () => {
  open();

  expect(await screen.findByText("Nothing needs you")).toBeVisible();
  expect(screen.getByText("Nothing to approve")).toBeVisible();
  expect(screen.getByRole("heading", { level: 1, name: "Overview" })).toBeVisible();
  expect(screen.getByText("What needs you and how your hosts are doing.")).toBeVisible();
});

test("needs_attention_lists_the_first_five_incidents_as_links", async () => {
  route("GET /api/v1/incidents", () => ({
    items: Array.from({ length: 7 }, (_, n) =>
      incident({
        id: `inc-${n}`,
        title: `Problem ${n}`,
        severity: n === 0 ? "critical" : "warning",
      }),
    ),
    next_cursor: null,
  }));
  open();

  await screen.findByText("Problem 0");
  const list = within(screen.getByRole("region", { name: "Needs attention" }));
  const links = list.getAllByRole("link").filter((link) => link.textContent !== "View all");
  expect(links, "five rows, not seven").toHaveLength(5);
  expect(links[0]).toHaveAttribute("href", "/incidents/inc-0");
  expect(links[0]).toHaveTextContent(/Critical.*Problem 0.*nas/);
  expect(list.getByRole("link", { name: "View all" })).toHaveAttribute("href", "/incidents");
  expect(
    calls.find((call) => call.route === "GET /api/v1/incidents")?.init.params?.query,
    "only incidents that need action are asked for",
  ).toMatchObject({ group: "needs_action" });
});

test("waiting_for_approval_lists_actions_with_skill_host_risk_and_a_link", async () => {
  route("GET /api/v1/actions", () => ({
    items: [
      action({ id: "late", skill: "later_one", expires_at: "2026-10-31T14:00:00Z" }),
      action({
        id: "soon",
        skill: "sooner_one",
        risk: "destructive",
        expires_at: "2026-10-31T13:00:00Z",
      }),
    ],
    next_cursor: null,
  }));
  open();

  await screen.findByText("sooner_one");
  const list = within(screen.getByRole("region", { name: "Waiting for approval" }));
  const rows = list.getAllByRole("link").filter((link) => link.textContent !== "View all");
  expect(rows[0], "the soonest to expire first").toHaveTextContent(/sooner_one.*nas.*Destructive/);
  expect(rows[1]).toHaveTextContent("later_one");
  expect(rows[0]).toHaveAttribute("href", "/approvals");
  expect(list.getByRole("link", { name: "View all" })).toHaveAttribute("href", "/approvals");
});

test("hosts_are_cards_that_link_to_the_host", async () => {
  open();

  const card = await screen.findByRole("link", { name: "nas" });
  expect(card).toHaveAttribute("href", "/hosts/nas");
  expect(screen.getByRole("link", { name: "pi" })).toHaveAttribute("href", "/hosts/pi");
  expect(screen.getByRole("region", { name: "Hosts" })).toBeVisible();
});

test("overview_follows_the_host_selector", async () => {
  open("/?host=pi");

  await screen.findByRole("link", { name: "pi" });
  expect(screen.queryByRole("link", { name: "nas" }), "only the chosen host's card").toBeNull();
  expect(calls.find((call) => call.route === "GET /api/v1/overview")?.init.params?.query).toEqual({
    host: "pi",
  });
  expect(
    calls.find((call) => call.route === "GET /api/v1/actions")?.init.params?.query,
  ).toMatchObject({ host: "pi" });
});

test("a_failed_load_is_an_error_with_retry_and_never_nothing_needs_you", async () => {
  route("GET /api/v1/overview", () => failure(500, "internal", "the hub is sick"));
  route("GET /api/v1/incidents", () => failure(500, "internal", "the hub is sick"));
  open();

  expect(await screen.findByText("Could not load the overview")).toBeVisible();
  expect(await screen.findByText("Could not load incidents")).toBeVisible();
  expect(screen.getAllByRole("button", { name: "Retry" }).length).toBeGreaterThan(0);
  expect(
    screen.queryByText("Nothing needs you"),
    "a failed load is never read as empty",
  ).toBeNull();
});

test("needs_attention_puts_the_most_severe_first_then_the_most_recent", async () => {
  route("GET /api/v1/incidents", () => ({
    items: [
      incident({
        id: "i1",
        title: "Info newest",
        severity: "info",
        last_firing_at: "2026-10-31T12:30:00Z",
      }),
      incident({
        id: "w1",
        title: "Warning old",
        severity: "warning",
        last_firing_at: "2026-10-31T11:00:00Z",
      }),
      incident({
        id: "c1",
        title: "Critical old",
        severity: "critical",
        last_firing_at: "2026-10-31T10:00:00Z",
      }),
      incident({
        id: "w2",
        title: "Warning new",
        severity: "warning",
        last_firing_at: "2026-10-31T12:00:00Z",
      }),
    ],
    next_cursor: null,
  }));
  open();

  await screen.findByText("Critical old");
  const list = within(screen.getByRole("region", { name: "Needs attention" }));
  const titles = list
    .getAllByRole("link")
    .filter((link) => link.textContent !== "View all")
    .map((link) => link.textContent);
  expect(titles[0]).toMatch(/Critical old/);
  expect(titles[1]).toMatch(/Warning new/);
  expect(titles[2]).toMatch(/Warning old/);
  expect(titles[3]).toMatch(/Info newest/);
});

const fine = (n: number) => ({ ...nas, id: `ok-${n}`, name: `fine-${n}`, open_incidents: 0 });

test("hosts_shows_only_hosts_with_problems_six_at_most_and_a_link_to_all", async () => {
  route("GET /api/v1/hosts", () => [
    ...Array.from({ length: 6 }, (_, n) => fine(n)),
    ...Array.from({ length: 8 }, (_, n) => ({
      ...nas,
      id: `bad-${n}`,
      name: `bad-${n}`,
      connected: false,
      open_incidents: 0,
    })),
  ]);
  open();

  const section = within(await screen.findByRole("region", { name: "Hosts" }));
  expect(await section.findAllByRole("link", { name: /^bad-/ })).toHaveLength(6);
  expect(section.queryByRole("link", { name: /^fine-/ }), "a fine host is not a card").toBeNull();
  expect(section.getByRole("link", { name: "View all 14 hosts" })).toHaveAttribute(
    "href",
    "/hosts",
  );
});

test("a_connected_host_with_open_incidents_or_an_old_agent_is_a_problem", async () => {
  route("GET /api/v1/hosts", () => [
    fine(1),
    { ...fine(2), name: "busy", open_incidents: 3 },
    { ...fine(3), name: "old", outdated: true },
  ]);
  open();

  const section = within(await screen.findByRole("region", { name: "Hosts" }));
  expect(await section.findByRole("link", { name: /^busy/ })).toBeVisible();
  expect(section.getByRole("link", { name: /^old/ })).toBeVisible();
  expect(section.queryByRole("link", { name: /^fine-1/ })).toBeNull();
});

test("when_every_host_is_fine_one_muted_line_says_so", async () => {
  route("GET /api/v1/hosts", () => [fine(1), fine(2), fine(3)]);
  open();

  const section = within(await screen.findByRole("region", { name: "Hosts" }));
  expect(await section.findByText("All 3 hosts are fine")).toBeVisible();
  expect(section.queryByRole("link", { name: /^fine-/ })).toBeNull();
  expect(section.getByRole("link", { name: "View all 3 hosts" })).toBeVisible();
});

test("a_long_host_name_in_a_waiting_row_is_cut_and_keeps_its_tooltip", async () => {
  const user = userEvent.setup();
  const long = "edge-router-gateway-frankfurt-primary-uplink-monitor-and-failover-probe";
  route("GET /api/v1/hosts", () => [{ ...nas, name: long }]);
  route("GET /api/v1/actions", () => ({ items: [action()], next_cursor: null }));
  open();

  await screen.findByText("restart_container");
  const approvals = within(screen.getByRole("region", { name: "Waiting for approval" }));
  const name = approvals.getByText(long);
  expect(name.className, "one line with an ellipsis").toMatch(/host/);
  await user.hover(name);
  expect(await screen.findByRole("tooltip")).toHaveTextContent(long);
});
