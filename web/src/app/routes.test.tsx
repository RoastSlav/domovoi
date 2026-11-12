import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { ToastProvider } from "../components";
import { failure, resetApi, route } from "../test/apiMock";
import { nas } from "../test/fixtures";
import { routeSettingsPage, settings } from "../test/settingsFixture";
import { AppRoutes } from "./routes";

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
  route("GET /api/v1/incidents", () => ({ items: [], next_cursor: null }));
  route("GET /api/v1/overview", () => ({
    incidents_action_needed: 0,
    incidents_open: 0,
    approvals_waiting: 0,
    hosts_offline: 0,
    hosts_total: 1,
  }));
  route("GET /api/v1/system/update", () => ({ current: "0.1.0", latest: null, newer: false }));
});

afterEach(() => {
  vi.unstubAllGlobals();
});

function renderApp(url: string) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <MemoryRouter initialEntries={[url]}>
          <AppRoutes />
        </MemoryRouter>
      </ToastProvider>
    </QueryClientProvider>,
  );
}

const heading = (name: string) => screen.findByRole("heading", { level: 1, name });

test("require_session_shows_the_shell_skeleton_not_nothing_while_the_session_loads", () => {
  route("GET /api/v1/session", () => new Promise(() => {}));
  const { container } = renderApp("/incidents");

  expect(screen.getByText("Loading the page…")).toBeInTheDocument();
  expect(container.querySelector("main"), "the page area is already there").not.toBeNull();
  expect(screen.queryByRole("heading", { level: 1 })).toBeNull();
});

test("an_unreachable_hub_says_so_with_retry_instead_of_a_blank_screen", async () => {
  route("GET /api/v1/session", () => failure(503, "unavailable", "the hub is starting"));
  renderApp("/incidents");

  expect(await screen.findByText("Could not reach the hub")).toBeVisible();
  route("GET /api/v1/session", () => ({ user: { username: "admin" }, csrf_token: "t" }));
  fireEvent.click(screen.getByRole("button", { name: "Retry" }));
  expect(await heading("Incidents")).toBeVisible();
});

test("the_first_page_names_the_tab_and_leaves_focus_alone", async () => {
  renderApp("/incidents");

  const title = await heading("Incidents");
  expect(document.title).toBe("Incidents · Domovoi");
  expect(title).not.toHaveFocus();
});

test("a_page_outside_the_first_download_loads_names_the_tab_and_takes_focus", async () => {
  renderApp("/incidents");
  await heading("Incidents");

  const nav = screen.getByRole("navigation", { name: "Main", hidden: true });
  fireEvent.click(within(nav).getByRole("link", { name: "Hosts", hidden: true }));

  const title = await heading("Hosts");
  await waitFor(() => expect(title).toHaveFocus());
  expect(document.title).toBe("Hosts · Domovoi");
});

test("the_root_address_opens_the_overview_inside_the_shell", async () => {
  renderApp("/");

  expect(await heading("Overview")).toBeVisible();
  expect(document.title).toBe("Overview · Domovoi");
  const nav = screen.getByRole("navigation", { name: "Main", hidden: true });
  expect(within(nav).getByRole("link", { name: "Overview", hidden: true })).toHaveAttribute(
    "aria-current",
    "page",
  );
});

test("an_unknown_address_shows_a_not_found_page_inside_the_shell", async () => {
  renderApp("/nowhere/at/all");

  expect(await heading("Page not found")).toBeVisible();
  expect(screen.getByRole("link", { name: "Go to incidents" })).toHaveAttribute(
    "href",
    "/incidents",
  );
  expect(screen.getByRole("navigation", { name: "Main", hidden: true })).toBeInTheDocument();
  await waitFor(() => expect(document.title).toBe("Page not found · Domovoi"));
});

test("an_old_settings_about_link_lands_on_the_merged_about_and_updates_section", async () => {
  routeSettingsPage();
  route("GET /api/v1/settings", () => settings());
  renderApp("/settings/about");

  expect(await screen.findByRole("region", { name: "About and updates" })).toBeVisible();
  expect(screen.queryByText("No such section")).toBeNull();
});
