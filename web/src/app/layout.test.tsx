import { act, fireEvent, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Live } from "../api/events";
import { calls, failure, resetApi, route } from "../test/apiMock";
import { nas } from "../test/fixtures";
import { renderAt } from "../test/render";
import { SELECTED_HOST_KEY } from "./HostContext";
import { Layout } from "./Layout";
import { SIDEBAR_KEY } from "./useSidebarRail";

vi.mock("../api/client", () => import("../test/apiMock"));

/** The live state the layout sees, set by a test instead of an event stream. */
const live = vi.hoisted(() => {
  let value: { paused: boolean; status: string } = { status: "live", paused: false };
  const listeners = new Set<() => void>();
  return {
    get: () => value,
    set(next: { paused: boolean; status: string }) {
      value = next;
      listeners.forEach((listener) => listener());
    },
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
});

vi.mock("../api/events", async (original) => {
  const { useSyncExternalStore } = await import("react");
  return {
    ...(await original<typeof import("../api/events")>()),
    useLiveEvents: () => useSyncExternalStore(live.subscribe, live.get) as Live,
  };
});

const QUIET = {
  incidents_action_needed: 0,
  incidents_open: 0,
  approvals_waiting: 0,
  hosts_offline: 0,
  hosts_total: 1,
};

beforeEach(() => {
  resetApi();
  live.set({ status: "live", paused: false });
  route("GET /api/v1/session", () => ({ user: { username: "admin" }, csrf_token: "t" }));
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/overview", () => QUIET);
  route("GET /api/v1/system/update", () => ({ current: "0.1.0", latest: null, newer: false }));
});

afterEach(() => {
  vi.unstubAllGlobals();
});

/** The hub's own machine, with an agent never enrolled. */
const hubHost = {
  ...nas,
  id: "host-hub",
  name: "home-server",
  kind: "local",
  connected: false,
  agent_enrolled: false,
  version: null,
  last_seen: null,
};

/** The hub's own machine whose agent enrolled and then went offline. */
const silentHub = { ...hubHost, id: "host-silent-hub", name: "workshop", agent_enrolled: true };

function renderLayout(url = "/incidents") {
  return renderAt(url, "/*", <Layout />);
}

/** Answers media queries as a screen of some width would; jsdom has none. */
function stubMedia(matches: boolean) {
  vi.stubGlobal("matchMedia", () => ({
    matches,
    addEventListener() {},
    removeEventListener() {},
  }));
}

// jsdom ignores the 768px media query, so the sidebar is the phone's closed sheet
// (display: none) unless a test opens it: queries inside it pass `hidden`.
const nav = () => screen.getByRole("navigation", { name: "Main", hidden: true });
const navLink = (name: string) => within(nav()).getByRole("link", { name, hidden: true });
const hostSelect = () => screen.getByRole("combobox", { name: "Host" });
const sheet = () => document.getElementById("sidebar-panel") as HTMLElement;
const menuButton = () => screen.getByRole("button", { name: "Menu" });

test("navigation_is_grouped_by_what_the_operator_is_doing", () => {
  renderLayout();

  const linksOf = (group: string) =>
    within(within(nav()).getByRole("group", { name: group, hidden: true }))
      .getAllByRole("link", { hidden: true })
      .map((link) => link.textContent);
  expect(linksOf("Respond")).toEqual(["Overview", "Incidents", "Approvals", "Chat"]);
  expect(linksOf("Fleet")).toEqual(["Hosts", "Skills", "Logs"]);
  expect(linksOf("Knowledge")).toEqual(["Known conditions", "Notes", "Digest"]);
  expect(linksOf("System")).toEqual(["Settings"]);
  expect(navLink("Incidents")).toHaveAttribute("href", "/incidents");
  expect(navLink("Overview")).toHaveAttribute("href", "/");
  expect(within(sheet()).getByRole("link", { name: "Domovoi", hidden: true })).toHaveAttribute(
    "href",
    "/",
  );
});

test("overview_is_current_only_on_the_root_address", () => {
  renderLayout("/incidents");
  expect(navLink("Overview")).not.toHaveAttribute("aria-current");
  expect(navLink("Incidents")).toHaveAttribute("aria-current", "page");
});

test("menu_button_opens_the_sheet_which_keeps_focus_inside_and_escape_returns_focus", async () => {
  const user = userEvent.setup();
  renderLayout();
  expect(menuButton()).toHaveAttribute("aria-controls", "sidebar-panel");
  expect(menuButton()).toHaveAttribute("aria-expanded", "false");
  expect(sheet()).toHaveAttribute("data-open", "false");

  await user.click(menuButton());
  expect(menuButton()).toHaveAttribute("aria-expanded", "true");
  expect(screen.getByRole("dialog", { name: "Menu" })).toBe(sheet());
  const first = within(sheet()).getByRole("link", { name: "Domovoi" });
  const last = within(sheet()).getByRole("button", { name: "Account menu" });
  expect(first, "focus moves into the sheet").toHaveFocus();

  await user.tab({ shift: true });
  expect(last, "back from the first control wraps to the last").toHaveFocus();
  await user.tab();
  expect(first, "on from the last control wraps to the first").toHaveFocus();

  await user.keyboard("{Escape}");
  expect(menuButton()).toHaveAttribute("aria-expanded", "false");
  expect(sheet()).toHaveAttribute("data-open", "false");
  expect(sheet()).not.toHaveAttribute("role");
  expect(menuButton()).toHaveFocus();
});

test("the_sheets_close_button_closes_it_and_returns_focus_to_menu", async () => {
  const user = userEvent.setup();
  renderLayout();
  await user.click(menuButton());

  await user.click(within(sheet()).getByRole("button", { name: "Close menu" }));

  expect(sheet()).toHaveAttribute("data-open", "false");
  expect(menuButton()).toHaveFocus();
});

test("navigation_sheet_closes_after_following_a_link", async () => {
  const user = userEvent.setup();
  renderLayout();
  await user.click(menuButton());

  await user.click(within(sheet()).getByRole("link", { name: "Hosts" }));

  expect(screen.getByTestId("location")).toHaveTextContent("/hosts");
  expect(menuButton()).toHaveAttribute("aria-expanded", "false");
});

test("collapsed_sidebar_names_its_links_in_tooltips_and_remembers_being_expanded", () => {
  stubMedia(true);
  renderLayout();
  const shell = document.querySelector("[data-rail]");
  expect(shell, "a tablet starts with the icon rail").toHaveAttribute("data-rail", "true");
  expect(navLink("Incidents"), "the link keeps its name").toBeInTheDocument();

  // jsdom will not focus inside the closed sheet, so the focus event is sent by hand.
  fireEvent.focusIn(navLink("Incidents"));
  expect(screen.getByRole("tooltip")).toHaveTextContent("Incidents");
  fireEvent.focusOut(navLink("Incidents"));
  fireEvent.focusIn(navLink("Known conditions"));
  expect(screen.getByRole("tooltip")).toHaveTextContent("Known conditions");

  fireEvent.click(screen.getByRole("button", { name: "Expand the sidebar", hidden: true }));
  expect(shell).toHaveAttribute("data-rail", "false");
  expect(localStorage.getItem(SIDEBAR_KEY)).toBe("expanded");
  fireEvent.focusOut(navLink("Known conditions"));
  fireEvent.focusIn(navLink("Incidents"));
  expect(screen.queryByRole("tooltip"), "a visible label needs no tooltip").toBeNull();
  expect(
    screen.getByRole("button", { name: "Collapse the sidebar", hidden: true }),
  ).toBeInTheDocument();
});

test("a_remembered_collapse_keeps_the_rail_on_a_wide_screen", () => {
  localStorage.setItem(SIDEBAR_KEY, "collapsed");
  // Wide, and (as every query answers alike here) nothing else decides: the choice does.
  stubMedia(true);
  renderLayout();

  expect(document.querySelector("[data-rail]")).toHaveAttribute("data-rail", "true");
});

test("choosing_a_host_in_the_selector_sets_the_host_param_and_all_hosts_clears_it", async () => {
  const user = userEvent.setup();
  renderLayout("/incidents?status=open");
  expect(hostSelect()).toHaveTextContent("All hosts");

  await user.click(hostSelect());
  await user.click(await screen.findByRole("option", { name: /^nas\b/ }));
  expect(screen.getByTestId("location")).toHaveTextContent("/incidents?status=open&host=nas");
  expect(localStorage.getItem(SELECTED_HOST_KEY)).toBe("nas");
  expect(hostSelect()).toHaveTextContent("nas");

  await user.click(hostSelect());
  await user.click(screen.getByRole("option", { name: "All hosts" }));
  expect(screen.getByTestId("location")).toHaveTextContent("/incidents?status=open");
  expect(screen.getByTestId("location")).not.toHaveTextContent("host=");
  expect(localStorage.getItem(SELECTED_HOST_KEY)).toBeNull();
  expect(hostSelect()).toHaveTextContent("All hosts");
});

test("selected_host_is_restored_from_storage_when_the_url_has_none", () => {
  localStorage.setItem(SELECTED_HOST_KEY, "nas");
  renderLayout("/incidents");

  expect(hostSelect()).toHaveTextContent("nas");
});

test("host_in_the_url_wins_over_the_remembered_one", () => {
  localStorage.setItem(SELECTED_HOST_KEY, "pi");
  renderLayout("/incidents?host=nas");

  expect(hostSelect()).toHaveTextContent("nas");
});

test("host_selector_shows_each_hosts_connection_and_tells_hub_only_from_offline", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/hosts", () => [nas, hubHost, silentHub]);
  renderLayout();

  await user.click(hostSelect());
  await screen.findByRole("option", { name: /^nas\b/ });
  expect(screen.getAllByRole("option").map((option) => option.textContent)).toEqual([
    "All hosts3",
    "nasConnected",
    "home-serverHub only, no agent",
    "workshopOffline",
  ]);
});

test("opening_a_page_about_one_host_selects_that_host_in_the_selector", () => {
  renderLayout("/hosts/nas");

  expect(hostSelect()).toHaveTextContent("nas");
  expect(hostSelect()).not.toHaveTextContent("All hosts");
});

test("leaving_a_host_page_brings_back_the_hosts_chosen_before", async () => {
  const user = userEvent.setup();
  renderLayout("/hosts/nas");
  expect(hostSelect()).toHaveTextContent("nas");

  await user.click(menuButton());
  await user.click(within(sheet()).getByRole("link", { name: "Incidents" }));

  expect(screen.getByTestId("location")).toHaveTextContent("/incidents");
  expect(hostSelect()).toHaveTextContent("All hosts");
  expect(localStorage.getItem(SELECTED_HOST_KEY)).toBeNull();
});

test("a_page_with_a_host_param_selects_that_host", () => {
  renderLayout("/skills?host=nas");

  expect(hostSelect()).toHaveTextContent("nas");
});

test("choosing_all_hosts_on_a_host_page_opens_the_host_list_not_a_stale_page", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/hosts", () => [nas, hubHost]);
  renderLayout("/hosts/nas");

  await user.click(hostSelect());
  await user.click(await screen.findByRole("option", { name: "All hosts" }));

  expect(screen.getByTestId("location")).toHaveTextContent(/^\/hosts$/);
  expect(hostSelect()).toHaveTextContent("All hosts");
  expect(localStorage.getItem(SELECTED_HOST_KEY)).toBeNull();
});

test("choosing_another_host_on_a_host_page_opens_that_hosts_page", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/hosts", () => [nas, hubHost]);
  renderLayout("/hosts/nas");

  await user.click(hostSelect());
  await user.click(await screen.findByRole("option", { name: /^home-server/ }));

  expect(screen.getByTestId("location")).toHaveTextContent("/hosts/home-server");
  expect(hostSelect()).toHaveTextContent("home-server");
});

test("ctrl_k_and_the_search_button_open_the_command_palette_state", async () => {
  const user = userEvent.setup();
  renderLayout();
  const search = () => screen.getByRole("button", { name: /^Search/, hidden: true });
  expect(search()).toHaveAttribute("aria-expanded", "false");

  await user.keyboard("{Control>}k{/Control}");
  expect(search()).toHaveAttribute("aria-expanded", "true");
  await user.keyboard("{Control>}k{/Control}");
  expect(search()).toHaveAttribute("aria-expanded", "false");

  fireEvent.click(search());
  expect(search()).toHaveAttribute("aria-expanded", "true");
});

test("host_selector_says_when_hosts_cannot_load_and_retry_loads_them", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/hosts", () => failure(503, "unavailable", "the hub is starting"));
  renderLayout();

  expect(await screen.findByText("Could not load hosts")).toBeVisible();
  expect(screen.queryByRole("combobox", { name: "Host" }), "not an empty list").toBeNull();

  route("GET /api/v1/hosts", () => [nas]);
  await user.click(screen.getByRole("button", { name: "Retry" }));
  await user.click(await screen.findByRole("combobox", { name: "Host" }));
  expect(await screen.findByRole("option", { name: /^nas\b/ })).toBeVisible();
});

test("approvals_count_follows_the_selected_host", async () => {
  route("GET /api/v1/overview", (init) => ({
    ...QUIET,
    approvals_waiting: init.params?.query?.host === "nas" ? 2 : 5,
  }));
  renderLayout("/incidents?host=nas");

  expect(navLink("Approvals")).toBeInTheDocument();
  expect(
    await within(nav()).findByRole("link", { name: "Approvals 2 waiting", hidden: true }),
  ).toBeInTheDocument();
  const asked = calls.filter((call) => call.route === "GET /api/v1/overview");
  expect(asked.map((call) => call.init.params?.query)).toEqual([{ host: "nas" }]);
});

test("counts_show_what_needs_attention_and_hide_at_zero", async () => {
  route("GET /api/v1/overview", () => ({
    ...QUIET,
    incidents_action_needed: 3,
    hosts_offline: 1,
  }));
  renderLayout();

  expect(
    await within(nav()).findByRole("link", { name: "Incidents 3 need attention", hidden: true }),
  ).toBeInTheDocument();
  expect(navLink("Hosts 1 offline")).toBeInTheDocument();
  expect(navLink("Approvals"), "nothing waits, so no count").toBeInTheDocument();
});

test("hosts_count_hides_when_no_host_is_offline", async () => {
  renderLayout();

  await screen.findByText("admin", { ignore: "[hidden]" });
  await act(async () => {});
  expect(navLink("Hosts"), "exactly Hosts: no count beside it").toBeInTheDocument();
});

test("a_long_outage_shows_one_banner_and_says_back_online_when_it_ends", async () => {
  renderLayout();
  expect(screen.queryByText("Live updates paused")).toBeNull();

  act(() => live.set({ status: "offline", paused: true }));
  expect(screen.getByText("Live updates paused")).toBeVisible();
  expect(screen.getByText("Reconnecting…")).toBeVisible();

  act(() => live.set({ status: "live", paused: false }));
  expect(screen.queryByText("Live updates paused")).toBeNull();
  expect(await screen.findByText("Back online")).toBeVisible();
});

test("a_short_outage_shows_no_banner_and_no_toast", () => {
  renderLayout();

  act(() => live.set({ status: "offline", paused: false }));
  act(() => live.set({ status: "live", paused: false }));

  expect(screen.queryByText("Live updates paused")).toBeNull();
  expect(screen.queryByText("Back online")).toBeNull();
});

test("breadcrumb_and_tab_title_name_the_section_and_the_thing", () => {
  renderLayout("/hosts/nas-01");

  // Hidden at phone width, which is all jsdom knows, so it is found by its label.
  const crumbs = document.querySelector<HTMLElement>('nav[aria-label="Breadcrumb"]');
  if (!crumbs) {
    throw new Error("no breadcrumb");
  }
  expect(within(crumbs).getByRole("link", { name: "Hosts", hidden: true })).toHaveAttribute(
    "href",
    "/hosts",
  );
  expect(within(crumbs).getByText("nas-01")).toHaveAttribute("aria-current", "page");
  expect(document.title).toBe("nas-01 · Hosts · Domovoi");
});

test("account_menu_names_the_user_switches_the_theme_and_logs_out", async () => {
  const user = userEvent.setup();
  route("POST /api/v1/auth/logout", () => undefined);
  renderLayout();
  await user.click(menuButton());
  await user.click(within(sheet()).getByRole("button", { name: "Account menu" }));

  expect(await screen.findByText("Signed in as admin")).toBeVisible();
  const theme = within(screen.getByRole("group", { name: "Theme" }));
  expect(theme.getByRole("menuitemradio", { name: "System" })).toBeChecked();
  await user.click(theme.getByRole("menuitemradio", { name: "Dark" }));
  expect(document.documentElement.dataset.theme).toBe("dark");

  await user.click(within(sheet()).getByRole("button", { name: "Account menu" }));
  await user.click(screen.getByRole("menuitem", { name: "Log out" }));
  expect(calls.some((call) => call.route === "POST /api/v1/auth/logout")).toBe(true);
});
