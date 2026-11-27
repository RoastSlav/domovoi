import { screen, within } from "@testing-library/react";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { incompatibleHost, offlineHost } from "../../test/attentionFixtures";
import { serveAttention } from "../../test/attentionRoutes";
import { nas, overview } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { hostSeverity, OverviewPage } from "./OverviewPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const pi = { ...nas, id: "host-pi", name: "pi", connected: false, open_incidents: 0 };

beforeEach(() => {
  resetApi();
  localStorage.clear();
  serveAttention({ hosts: [nas, pi] });
  route("GET /api/v1/overview", () => overview());
});

function open(url = "/") {
  return renderAt(url, "/", <OverviewPage />);
}

const tile = (name: RegExp) => screen.findByRole("link", { name });

test("tiles_show_attention_running_approvals_and_hosts_and_lead_to_their_lists", async () => {
  route("GET /api/v1/overview", () =>
    overview({
      attention_total: 6,
      operations_running: 1,
      operations_waiting_approval: 3,
    }),
  );
  open();

  expect(await tile(/^Needs you 6/)).toHaveAttribute("href", "/attention");
  expect(await tile(/^Running now 1/)).toHaveAttribute("href", "/operations?status=running");
  expect(await tile(/^Waiting for you 3/)).toHaveAttribute("href", "/attention");
  const hosts = await tile(/^Hosts online 1 of 2/);
  expect(hosts).toHaveAttribute("href", "/hosts");
  expect(hosts, "what is down is said in words, not only by colour").toHaveTextContent("1 offline");
});

test("the_waiting_tile_counts_questions_as_well_as_approvals", async () => {
  route("GET /api/v1/overview", () =>
    overview({ operations_waiting_approval: 1, operations_waiting_user: 2 }),
  );
  open();

  const waiting = await tile(/^Waiting for you 3/);
  expect(waiting).toHaveTextContent("1 to approve, 2 questions to answer");
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
  serveAttention({ hosts: [nas] });
  open();

  expect(await tile(/^Needs you 0/)).toHaveTextContent("All clear");
  expect(await tile(/^Running now 0/)).toHaveTextContent("Nothing running");
  expect(await tile(/^Hosts online 1 of 1/)).toHaveTextContent("All connected");
});

test("a_missing_attention_count_reads_neutral_not_all_clear", async () => {
  route("GET /api/v1/overview", () => {
    const old: Partial<ReturnType<typeof overview>> = overview();
    delete old.attention_total;
    return old;
  });
  open();

  const needs = await tile(/^Needs you/);
  expect(needs, "an older hub sends no count").not.toHaveTextContent("All clear");
});

test("control_room_has_its_title_and_intro", async () => {
  serveAttention({ hosts: [nas] });
  open();

  expect(screen.getByRole("heading", { level: 1, name: "Control room" })).toBeVisible();
  expect(
    screen.getByText("What is running, what needs you and how your hosts are doing."),
  ).toBeVisible();
});

test("nothing_needs_you_and_nothing_to_approve_are_said_when_both_are_empty", async () => {
  serveAttention({ hosts: [nas] });
  open();

  expect(await screen.findByText("Nothing needs you")).toBeVisible();
  expect(screen.getByText("Nothing to approve")).toBeVisible();
  expect(await screen.findByText("Nothing is running")).toBeVisible();
});

test("hosts_are_cards_that_link_to_the_host", async () => {
  open();

  const card = await screen.findByRole("link", { name: "nas" });
  expect(card).toHaveAttribute("href", "/hosts/nas");
  expect(screen.getByRole("link", { name: "pi" })).toHaveAttribute("href", "/hosts/pi");
  expect(screen.getByRole("region", { name: "Hosts" })).toBeVisible();
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

test("an_incompatible_host_is_among_the_cards_and_never_called_offline", async () => {
  serveAttention({
    hosts: [
      ...Array.from({ length: 6 }, (_, n) => offlineHost(`down-${n}`, { open_incidents: 9 })),
      incompatibleHost("old-agent"),
    ],
  });
  open();

  const section = within(await screen.findByRole("region", { name: "Hosts" }));
  const card = await section.findByRole("link", { name: /^old-agent/ });
  expect(card, "ahead of six offline hosts, so it is within the six").toHaveTextContent(
    "Incompatible",
  );
  expect(card).not.toHaveTextContent("Offline");
  expect(section.getAllByRole("link", { name: /^(down|old)-/ })).toHaveLength(6);
});

test("host_selection_ranks_incompatible_then_offline_then_expected_incompatible", () => {
  const hosts = [
    { ...nas, name: "connected-busy", open_incidents: 9 },
    offlineHost("never", { agent_enrolled: false }),
    offlineHost("expected", { protocol_state: "expected_incompatible" }),
    offlineHost("down-few", { open_incidents: 1 }),
    offlineHost("down-many", { open_incidents: 4 }),
    incompatibleHost("old"),
  ];

  expect(hosts.sort(hostSeverity).map((host) => host.name)).toEqual([
    "old",
    "down-many",
    "down-few",
    "expected",
    "never",
    "connected-busy",
  ]);
});

test("control_room_follows_the_host_selector", async () => {
  open("/?host=pi");

  await screen.findByRole("link", { name: "pi" });
  expect(screen.queryByRole("link", { name: "nas" }), "only the chosen host's card").toBeNull();
  expect(calls.find((call) => call.route === "GET /api/v1/overview")?.init.params?.query).toEqual({
    host: "pi",
  });
  const sources = calls.filter((call) => /\/(incidents|operations)$/.test(call.route));
  expect(sources.length).toBeGreaterThanOrEqual(5);
  for (const call of sources) {
    expect(call.init.params?.query, call.route).toMatchObject({ host: "pi" });
  }
});

test("a_failed_overview_is_a_load_error_with_retry", async () => {
  route("GET /api/v1/overview", () => failure(500, "internal", "the hub is sick"));
  open();

  expect(await screen.findByText("Could not load the control room")).toBeVisible();
  expect(screen.getAllByRole("button", { name: "Retry" }).length).toBeGreaterThan(0);
});
