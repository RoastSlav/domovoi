import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { calls, resetApi, route } from "../test/apiMock";
import { action, nas } from "../test/fixtures";
import { renderAt } from "../test/render";
import { SELECTED_HOST_KEY } from "./HostContext";
import { Layout } from "./Layout";

vi.mock("../api/client", () => import("../test/apiMock"));

beforeEach(() => {
  resetApi();
  // jsdom has no event stream; the layout only needs it not to throw.
  vi.stubGlobal(
    "EventSource",
    class {
      addEventListener() {}
      close() {}
    },
  );
  route("GET /api/v1/session", () => ({ user: { username: "admin" }, csrf_token: "t" }));
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/actions", () => ({ items: [] }));
  route("GET /api/v1/system/update", () => ({
    current: "0.1.0",
    latest: null,
    newer: false,
  }));
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
const silentHub = {
  ...hubHost,
  id: "host-silent-hub",
  name: "workshop",
  agent_enrolled: true,
};

function renderLayout(url = "/incidents") {
  return renderAt(url, "/*", <Layout />);
}

const hostSelect = () => screen.getByRole("combobox", { name: "Host" });
const sheet = () => document.getElementById("sidebar-panel");

test("sidebar_lists_the_main_navigation_links", () => {
  renderLayout();

  // jsdom ignores the 900px media query, so the sheet is closed (display: none) here; on a
  // wide screen the same nav is always shown.
  const nav = screen.getByRole("navigation", { name: "Main", hidden: true });
  const links = within(nav).getAllByRole("link", { hidden: true });
  expect(links.map((link) => link.textContent)).toEqual([
    "Incidents",
    "Chat",
    "Known conditions",
    "Approvals",
    "Hosts",
    "Skills",
    "Notes",
    "Digest",
    "Logs",
    "Settings",
  ]);
  expect(links[0]).toHaveAttribute("href", "/incidents");
});

test("menu_button_opens_the_navigation_sheet_and_escape_closes_it", async () => {
  const user = userEvent.setup();
  renderLayout();
  const menu = screen.getByRole("button", { name: "Menu" });
  expect(menu).toHaveAttribute("aria-controls", "sidebar-panel");
  expect(menu).toHaveAttribute("aria-expanded", "false");
  expect(sheet()).toHaveAttribute("data-open", "false");

  await user.click(menu);
  expect(menu).toHaveAttribute("aria-expanded", "true");
  expect(sheet()).toHaveAttribute("data-open", "true");

  await user.keyboard("{Escape}");
  expect(menu).toHaveAttribute("aria-expanded", "false");
  expect(sheet()).toHaveAttribute("data-open", "false");
  expect(menu).toHaveFocus();
});

test("navigation_sheet_closes_after_following_a_link", async () => {
  const user = userEvent.setup();
  renderLayout();
  await user.click(screen.getByRole("button", { name: "Menu" }));

  await user.click(screen.getByRole("link", { name: "Hosts" }));

  expect(screen.getByTestId("location")).toHaveTextContent("/hosts");
  expect(screen.getByRole("button", { name: "Menu" })).toHaveAttribute("aria-expanded", "false");
});

test("choosing_a_host_in_the_selector_sets_the_host_param_and_all_hosts_clears_it", async () => {
  const user = userEvent.setup();
  renderLayout("/incidents?status=open");
  await screen.findByRole("option", { name: "● nas" });

  await user.selectOptions(hostSelect(), "nas");
  expect(screen.getByTestId("location")).toHaveTextContent("/incidents?status=open&host=nas");
  expect(localStorage.getItem(SELECTED_HOST_KEY)).toBe("nas");

  await user.selectOptions(hostSelect(), "All hosts");
  expect(screen.getByTestId("location")).toHaveTextContent("/incidents?status=open");
  expect(screen.getByTestId("location")).not.toHaveTextContent("host=");
  expect(localStorage.getItem(SELECTED_HOST_KEY)).toBeNull();
  expect(hostSelect()).toHaveValue("");
});

test("selected_host_is_restored_from_storage_when_the_url_has_none", async () => {
  localStorage.setItem(SELECTED_HOST_KEY, "nas");
  renderLayout("/incidents");

  await screen.findByRole("option", { name: "● nas" });
  expect(hostSelect()).toHaveValue("nas");
});

test("host_in_the_url_wins_over_the_remembered_one", async () => {
  localStorage.setItem(SELECTED_HOST_KEY, "pi");
  route("GET /api/v1/hosts", () => [nas, { ...nas, id: "host-pi", name: "pi" }]);
  renderLayout("/incidents?host=nas");

  await screen.findByRole("option", { name: "● pi" });
  expect(hostSelect()).toHaveValue("nas");
});

test("host_selector_marks_connected_hosts_and_tells_hub_only_from_offline", async () => {
  route("GET /api/v1/hosts", () => [nas, hubHost, silentHub]);
  renderLayout();

  await screen.findByRole("option", { name: "● nas" });
  const options = within(hostSelect())
    .getAllByRole("option")
    .map((option) => option.textContent);
  expect(options).toEqual([
    "All hosts",
    "● nas",
    "○ home-server (hub only)",
    "○ workshop (offline)",
  ]);
});

test("approvals_link_shows_how_many_actions_are_proposed", async () => {
  route("GET /api/v1/actions", () => ({ items: [action(), action({ id: "act-2" })] }));
  renderLayout();

  expect(
    await screen.findByRole("link", { name: "Approvals 2 waiting", hidden: true }),
  ).toBeInTheDocument();
  expect(calls.find((call) => call.route === "GET /api/v1/actions")?.init.params?.query).toEqual({
    status: "proposed",
    limit: 200,
  });
});

test("approvals_link_has_no_badge_when_nothing_is_proposed", async () => {
  renderLayout();

  await screen.findByText("admin");
  expect(screen.getByRole("link", { name: "Approvals", hidden: true })).toBeInTheDocument();
});
