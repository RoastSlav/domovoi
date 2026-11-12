import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Host } from "../../api/types";
import { resetApi, route } from "../../test/apiMock";
import { nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { countsOf, filterHosts } from "./hostFilter";
import { HostsPage } from "./HostsPage";
import { sortHosts, DEFAULT_HOST_SORT } from "./hostSort";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const HOSTS = "GET /api/v1/hosts";

beforeEach(() => {
  resetApi();
  try {
    localStorage.setItem("domovoi.hosts-view", "table");
  } catch {
    // Storage blocked: the page opens on cards and the tests below read both views.
  }
});

function host(name: string, overrides: Partial<Host> = {}): Host {
  return { ...nas, id: `host-${name}`, name, kind: "agent", open_incidents: 0, ...overrides };
}

const down = (name: string) =>
  host(name, { connected: false, version: null, last_seen: "2026-11-01T09:00:00Z" });
const never = (name: string) =>
  host(name, { connected: false, version: null, last_seen: null, agent_enrolled: false });
const old = (name: string) => host(name, { outdated: true, version: "0.0.1" });

const FLEET = [host("a"), host("b"), down("c"), never("d"), old("e"), host("f")];

function names(): string[] {
  const table = screen.getByRole("table", { name: "Hosts" });
  return within(table)
    .getAllByRole("row")
    .slice(1)
    .map((row) => within(row).getAllByRole("cell")[0]?.textContent ?? "");
}

test("problems_are_offline_never_connected_refused_or_outdated_hosts", () => {
  expect(countsOf(FLEET)).toEqual({ all: 6, problems: 3, online: 4, offline: 2 });
  expect(filterHosts(FLEET, "problems", "").map((h) => h.name)).toEqual(["c", "d", "e"]);
  expect(filterHosts(FLEET, "offline", "").map((h) => h.name)).toEqual(["c", "d"]);
  expect(filterHosts(FLEET, "online", "").map((h) => h.name)).toEqual(["a", "b", "e", "f"]);
  expect(filterHosts(FLEET, "all", "  B ").map((h) => h.name)).toEqual(["b"]);
});

test("problem_hosts_sort_before_healthy_ones", () => {
  expect(sortHosts(FLEET, DEFAULT_HOST_SORT).map((h) => h.name)).toEqual([
    "c",
    "d",
    "e",
    "a",
    "b",
    "f",
  ]);
});

test("the_filter_chips_narrow_the_list_and_show_how_many_hosts_each_one_holds", async () => {
  route(HOSTS, () => FLEET);
  const user = userEvent.setup();
  renderAt("/hosts", "/hosts", <HostsPage />);
  await screen.findByRole("table", { name: "Hosts" });

  const chips = screen.getByRole("group", { name: "Show hosts" });
  expect(
    within(chips).getByRole("radio", { name: /Problems.*3 hosts with problems/ }),
  ).toBeVisible();
  await user.click(within(chips).getByRole("radio", { name: /^Problems/ }));
  expect(names()).toEqual(["c", "d", "e"]);
  await user.click(within(chips).getByRole("radio", { name: /^Online/ }));
  expect(names()).toEqual(["e", "a", "b", "f"]);
  await user.click(within(chips).getByRole("radio", { name: /^All/ }));
  expect(names()).toHaveLength(6);
  expect(screen.queryByRole("searchbox", { name: "Search hosts" })).toBeNull();
});

test("with_more_than_eight_hosts_a_search_box_finds_hosts_by_name", async () => {
  route(HOSTS, () => Array.from({ length: 10 }, (_, i) => host(`node-${i}`)));
  const user = userEvent.setup();
  renderAt("/hosts", "/hosts", <HostsPage />);
  await screen.findByRole("table", { name: "Hosts" });

  await user.type(screen.getByRole("searchbox", { name: "Search hosts" }), "node-7");
  expect(names()).toEqual(["node-7"]);
  await user.clear(screen.getByRole("searchbox", { name: "Search hosts" }));
  await user.type(screen.getByRole("searchbox", { name: "Search hosts" }), "zzz");
  expect(await screen.findByText("No hosts match")).toBeVisible();
});

test("a_long_host_name_stops_after_two_lines_and_keeps_its_whole_name", async () => {
  const long = "media-server-living-room-with-a-very-long-name-for-layout-testing-01";
  route(HOSTS, () => [host(long)]);
  renderAt("/hosts", "/hosts", <HostsPage />);
  const table = await screen.findByRole("table", { name: "Hosts" });
  expect(table).toHaveAttribute("data-layout", "fixed");
  const cell = within(table).getAllByRole("cell")[0] as HTMLElement;
  expect(cell).toHaveAttribute("data-truncate", "clamp2");
  expect(cell).toHaveTextContent(long);
});

test("an_external_host_is_never_offered_an_agent_to_enroll", async () => {
  localStorage.setItem("domovoi.hosts-view", "cards");
  route(HOSTS, () => [
    host("uptime-kuma-bridge", {
      kind: "external",
      agent_enrolled: false,
      connected: false,
      version: null,
      last_seen: null,
    }),
    never("pi"),
  ]);
  renderAt("/hosts", "/hosts", <HostsPage />);
  const bridge = (await screen.findByRole("link", { name: "uptime-kuma-bridge" })).parentElement;
  expect(within(bridge as HTMLElement).queryByRole("button", { name: "Enroll agent" })).toBeNull();
  const pi = screen.getByRole("link", { name: "pi" }).parentElement as HTMLElement;
  expect(within(pi).getByRole("button", { name: "Enroll agent" })).toBeVisible();
});
