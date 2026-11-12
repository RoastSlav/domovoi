import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Host } from "../../api/types";
import { resetApi, route } from "../../test/apiMock";
import { nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { HostsPage } from "./HostsPage";
import { HOSTS_VIEW_KEY } from "./useHostsView";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const NOW = new Date("2026-11-01T12:00:00Z");
const HOSTS = "GET /api/v1/hosts";

beforeEach(resetApi);

afterEach(() => {
  vi.useRealTimers();
});

function host(name: string, overrides: Partial<Host> = {}): Host {
  return { ...nas, id: `host-${name}`, name, kind: "agent", open_incidents: 0, ...overrides };
}

function offline(name: string, overrides: Partial<Host> = {}): Host {
  return host(name, {
    connected: false,
    version: null,
    last_seen: "2026-11-01T09:00:00Z",
    ...overrides,
  });
}

/** The card of host `name`: the link and the menu button that sits beside it. */
function card(name: string): HTMLElement {
  const link = screen.getByRole("link", { name });
  return link.parentElement as HTMLElement;
}

test("the_hosts_open_as_cards_with_the_offline_ones_first", async () => {
  route(HOSTS, () => [host("alpha"), offline("bravo")]);
  renderAt("/hosts", "/hosts", <HostsPage />);

  await screen.findByRole("link", { name: "alpha" });
  const names = screen
    .getAllByRole("link")
    .filter((link) => link.getAttribute("href")?.startsWith("/hosts/"))
    .map((link) => link.getAttribute("aria-label"));
  expect(names).toEqual(["bravo", "alpha"]);
  expect(screen.queryByRole("table")).toBeNull();
});

test("a_host_card_says_what_state_the_host_is_in_at_a_glance", async () => {
  vi.useFakeTimers({ now: NOW, shouldAdvanceTime: true });
  route(HOSTS, () => [
    host("live", {
      kind: "container_agent",
      version: "0.0.8",
      connected_since: "2026-10-29T12:00:00Z",
      open_incidents: 2,
    }),
    offline("gone"),
  ]);
  renderAt("/hosts", "/hosts", <HostsPage />);
  await screen.findByRole("link", { name: "live" });

  const live = card("live");
  expect(within(live).getByText("Connected")).toBeVisible();
  expect(within(live).getByText("Container agent")).toBeVisible();
  expect(within(live).getByText("0.0.8")).toBeVisible();
  expect(within(live).getByText("3 d")).toBeVisible();
  expect(within(live).getByText(/^Connected for/)).toBeVisible();
  expect(within(live).getByText("2 open incidents")).toBeInTheDocument();
  expect(within(live).queryByText("No open incidents")).toBeNull();

  const gone = card("gone");
  expect(within(gone).getByText("Offline")).toBeVisible();
  expect(within(gone).getByText("3 hours ago")).toBeVisible();
  expect(within(gone).getByText("No open incidents")).toBeVisible();
});

test("the_whole_card_links_to_the_host_and_its_menu_is_not_inside_the_link", async () => {
  const user = userEvent.setup();
  route(HOSTS, () => [host("nas 01")]);
  renderAt("/hosts", "/hosts", <HostsPage />);

  const link = await screen.findByRole("link", { name: "nas 01" });
  expect(link).toHaveAttribute("href", "/hosts/nas%2001");
  const menu = screen.getByRole("button", { name: "Actions for nas 01" });
  expect(link.contains(menu)).toBe(false);

  await user.click(menu);
  await user.click(screen.getByRole("menuitem", { name: /Copy name/ }));
  expect(await screen.findByText("Copied nas 01")).toBeVisible();
});

test("the_chosen_view_is_remembered_between_visits", async () => {
  const user = userEvent.setup();
  route(HOSTS, () => [host("nas")]);
  const first = renderAt("/hosts", "/hosts", <HostsPage />);

  await screen.findByRole("link", { name: "nas" });
  await user.click(screen.getByRole("radio", { name: "Table" }));
  expect(await screen.findByRole("table", { name: "Hosts" })).toBeVisible();
  expect(localStorage.getItem(HOSTS_VIEW_KEY)).toBe("table");

  first.unmount();
  renderAt("/hosts", "/hosts", <HostsPage />);
  expect(await screen.findByRole("table", { name: "Hosts" })).toBeVisible();
  await user.click(screen.getByRole("radio", { name: "Cards" }));
  expect(screen.queryByRole("table")).toBeNull();
});

test("a_blocked_storage_still_lets_the_view_change", async () => {
  const user = userEvent.setup();
  route(HOSTS, () => [host("nas")]);
  const get = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
    throw new Error("blocked");
  });
  const set = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new Error("blocked");
  });
  try {
    renderAt("/hosts", "/hosts", <HostsPage />);
    await screen.findByRole("link", { name: "nas" });
    await user.click(screen.getByRole("radio", { name: "Table" }));
    expect(await screen.findByRole("table", { name: "Hosts" })).toBeVisible();
  } finally {
    get.mockRestore();
    set.mockRestore();
  }
});

test("a_host_without_an_agent_offers_to_enroll_one_instead_of_repeating_its_pill", async () => {
  const user = userEvent.setup();
  route(HOSTS, () => [
    host("pi", { agent_enrolled: false, connected: false, version: null, last_seen: null }),
    host("hub", { kind: "local", agent_enrolled: false, connected: false, version: null }),
  ]);
  renderAt("/hosts", "/hosts", <HostsPage />);
  await screen.findByRole("link", { name: "pi" });

  const pi = card("pi");
  expect(within(pi).getAllByText("Never connected")).toHaveLength(1);
  expect(screen.queryByText("No agent on this machine")).toBeNull();
  const enroll = within(pi).getByRole("button", { name: "Enroll agent" });
  expect(screen.getByRole("link", { name: "pi" }).contains(enroll)).toBe(false);
  expect(within(card("hub")).getByRole("button", { name: "Enroll agent" })).toBeVisible();

  await user.click(enroll);
  const dialog = screen.getByRole("dialog", { name: "Add a host" });
  expect(within(dialog).getByLabelText("Host name")).toHaveValue("pi");
});

test("one_open_incident_is_not_called_incidents", async () => {
  route(HOSTS, () => [host("one", { open_incidents: 1 }), host("two", { open_incidents: 2 })]);
  renderAt("/hosts", "/hosts", <HostsPage />);
  await screen.findByRole("link", { name: "one" });
  expect(within(card("one")).getByText("1 open incident")).toBeInTheDocument();
  expect(within(card("two")).getByText("2 open incidents")).toBeInTheDocument();
});
