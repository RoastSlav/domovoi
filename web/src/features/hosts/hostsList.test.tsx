import { act, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Host } from "../../api/types";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { HostsPage } from "./HostsPage";
import { HOSTS_VIEW_KEY } from "./useHostsView";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const NOW = new Date("2026-11-01T12:00:00Z");
const HOSTS = "GET /api/v1/hosts";
const REVOKE = "POST /api/v1/hosts/{name}/revoke";

// The table is the view most of these read; the card view has its own tests in hostCards.test.tsx.
beforeEach(() => {
  resetApi();
  localStorage.setItem(HOSTS_VIEW_KEY, "table");
});

afterEach(() => {
  vi.useRealTimers();
});

/** An agent host that is connected, overridable. */
function host(name: string, overrides: Partial<Host> = {}): Host {
  return { ...nas, id: `host-${name}`, name, kind: "agent", open_incidents: 0, ...overrides };
}

/** A host whose agent dropped three hours before `NOW`. */
function offline(name: string, overrides: Partial<Host> = {}): Host {
  return host(name, {
    connected: false,
    version: null,
    last_seen: "2026-11-01T09:00:00Z",
    ...overrides,
  });
}

function count(key: string) {
  return calls.filter((c) => c.route === key).length;
}

/** The host names in the table, top to bottom. */
function order(): string[] {
  return within(screen.getByRole("table", { name: "Hosts" }))
    .getAllByRole("link")
    .filter((link) => link.getAttribute("href")?.startsWith("/hosts/"))
    .map((link) => link.textContent);
}

/** The table row of host `name`. */
function row(name: string): HTMLElement {
  const link = within(screen.getByRole("table", { name: "Hosts" })).getByRole("link", { name });
  return link.closest("tr") as HTMLElement;
}

async function openMenu(user: ReturnType<typeof userEvent.setup>, name: string, item: string) {
  await user.click(await screen.findByRole("button", { name: `Actions for ${name}` }));
  await user.click(screen.getByRole("menuitem", { name: new RegExp(item) }));
}

test("hosts_page_explains_which_hosts_it_knows", async () => {
  route(HOSTS, () => [nas]);
  renderAt("/hosts", "/hosts", <HostsPage />);

  expect(
    await screen.findByText(/^Machines Domovoi watches, with an agent or through webhooks\./),
  ).toBeVisible();
  expect(
    screen.getByRole("link", { name: "Learn more about hosts (opens in a new tab)" }),
  ).toHaveAttribute("href", "https://RoastSlav.github.io/domovoi/concepts/hub-and-agent.html");
});

test("hosts_page_counts_hosts_online_and_offline", async () => {
  route(HOSTS, () => [host("a"), host("b"), offline("c")]);
  renderAt("/hosts", "/hosts", <HostsPage />);

  expect(await screen.findByText("3 hosts")).toBeVisible();
  expect(screen.getByText("2 online")).toBeVisible();
  expect(screen.getByText("1 offline")).toBeVisible();
});

test("offline_hosts_sort_first", async () => {
  route(HOSTS, () => [host("alpha"), offline("bravo"), host("charlie"), offline("delta")]);
  const user = userEvent.setup();
  renderAt("/hosts", "/hosts", <HostsPage />);

  await screen.findByRole("table", { name: "Hosts" });
  expect(order()).toEqual(["bravo", "delta", "alpha", "charlie"]);
  expect(screen.getByRole("columnheader", { name: "Connection" })).toHaveAttribute(
    "aria-sort",
    "ascending",
  );

  await user.click(screen.getByRole("button", { name: "Host" }));
  expect(order()).toEqual(["alpha", "bravo", "charlie", "delta"]);
  expect(screen.getByRole("columnheader", { name: "Host" })).toHaveAttribute(
    "aria-sort",
    "ascending",
  );
  expect(screen.getByRole("columnheader", { name: "Connection" })).toHaveAttribute(
    "aria-sort",
    "none",
  );

  await user.click(screen.getByRole("button", { name: "Host" }));
  expect(order()).toEqual(["delta", "charlie", "bravo", "alpha"]);
  expect(screen.getByRole("columnheader", { name: "Host" })).toHaveAttribute(
    "aria-sort",
    "descending",
  );
});

test("host_link_is_encoded", async () => {
  route(HOSTS, () => [host("nas 01")]);
  renderAt("/hosts", "/hosts", <HostsPage />);

  expect(await screen.findByRole("link", { name: "nas 01" })).toHaveAttribute(
    "href",
    "/hosts/nas%2001",
  );
});

test("each_host_shows_its_connection_kind_version_and_last_seen", async () => {
  vi.useFakeTimers({ now: NOW, shouldAdvanceTime: true });
  route(HOSTS, () => [
    host("live", { kind: "container_agent", version: "0.0.8" }),
    offline("gone"),
    offline("fresh", { agent_enrolled: false, last_seen: null }),
  ]);
  renderAt("/hosts", "/hosts", <HostsPage />);
  await screen.findByRole("table", { name: "Hosts" });

  expect(within(row("live")).getByText("Connected")).toBeVisible();
  expect(within(row("live")).getByText("Container agent")).toBeVisible();
  expect(within(row("live")).getByText("0.0.8")).toBeVisible();
  expect(within(row("gone")).getByText("Offline")).toBeVisible();
  expect(within(row("gone")).getByText("3 hours ago")).toBeVisible();
  expect(within(row("live")).getByText("now")).toBeVisible();
  expect(within(row("fresh")).getByText("Never connected")).toBeVisible();
});

test("outdated_agent_is_marked_update_available_and_links_to_its_incidents", async () => {
  route(HOSTS, () => [
    host("nas", { version: "0.0.0", outdated: true, open_incidents: 1 }),
    host("pi", { version: "0.0.1" }),
  ]);
  renderAt("/hosts", "/hosts", <HostsPage />);
  await screen.findByRole("table", { name: "Hosts" });

  expect(within(row("nas")).getByText("Update available")).toBeVisible();
  expect(within(row("nas")).getByRole("link", { name: "1 open incident on nas" })).toHaveAttribute(
    "href",
    "/incidents?host=nas",
  );
  expect(within(row("pi")).queryByText("Update available")).toBeNull();
  expect(within(row("pi")).queryByRole("link", { name: /open incidents/ })).toBeNull();
});

test("refused_agent_names_why_it_was_refused", async () => {
  route(HOSTS, () => [
    offline("nas", {
      last_seen: null,
      refused: "agent 0.0.1 is older than the minimum 0.0.5; update it",
    }),
  ]);
  renderAt("/hosts", "/hosts", <HostsPage />);
  await screen.findByRole("table", { name: "Hosts" });

  expect(
    within(row("nas")).getByText("agent 0.0.1 is older than the minimum 0.0.5; update it"),
  ).toBeVisible();
  expect(within(row("nas")).getByText("Refused")).toBeVisible();
  expect(within(row("nas")).queryByText("Update available")).toBeNull();
});

test("outdated_agent_opens_the_update_commands", async () => {
  const user = userEvent.setup();
  route(HOSTS, () => [host("nas", { version: "0.0.1", outdated: true })]);
  route("GET /api/v1/system/update", () => ({
    current: "0.0.7",
    latest: "v0.9.9",
    newer: true,
    hub_command: "install --role hub --version v0.9.9",
    agent_command: "install --role agent --version v0.9.9",
    docker_command: "DOMOVOI_IMAGE_TAG=0.9.9 docker compose -f hub.yml pull",
  }));
  renderAt("/hosts", "/hosts", <HostsPage />);

  await openMenu(user, "nas", "How to update");

  expect(screen.getByRole("dialog", { name: "How to update Domovoi" })).toBeVisible();
  expect(screen.getByLabelText("Agent update command")).toHaveTextContent("--role agent");
});

test("the_row_menu_opens_a_host_and_copies_its_name", async () => {
  const user = userEvent.setup();
  route(HOSTS, () => [host("nas")]);
  renderAt("/hosts", "/hosts", <HostsPage />);

  await openMenu(user, "nas", "Copy name");
  expect(await screen.findByText("Copied nas")).toBeVisible();
  expect(await navigator.clipboard.readText()).toBe("nas");

  await openMenu(user, "nas", "Open");
  expect(screen.getByTestId("location")).toHaveTextContent("/hosts/nas");
});

test("revoke_asks_for_the_typed_name_and_reports_the_result", async () => {
  const user = userEvent.setup();
  let revoked = false;
  route(HOSTS, () => [host("nas", { connected: !revoked })]);
  route(REVOKE, () => {
    revoked = true;
    return { revoked: true };
  });
  renderAt("/hosts", "/hosts", <HostsPage />);

  await openMenu(user, "nas", "Revoke agent");
  const dialog = screen.getByRole("dialog", { name: "Revoke the agent on nas?" });
  expect(dialog).toHaveTextContent("disconnects now");
  await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(count(REVOKE)).toBe(0);

  await openMenu(user, "nas", "Revoke agent");
  const confirm = within(screen.getByRole("dialog")).getByRole("button", { name: "Revoke agent" });
  expect(confirm, "the name must be typed first").toHaveAttribute("aria-disabled", "true");
  await user.click(confirm);
  expect(count(REVOKE)).toBe(0);

  await user.type(within(screen.getByRole("dialog")).getByLabelText(/Type nas to confirm/), "nas");
  await user.click(confirm);

  expect(calls.filter((c) => c.route === REVOKE).map((c) => c.init.params)).toEqual([
    { path: { name: "nas" } },
  ]);
  expect(await screen.findByText("Agent on nas revoked")).toBeVisible();
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(
    await within(row("nas")).findByText("Offline"),
    "the list is read again after the revoke",
  ).toBeVisible();
});

test("a_failed_revoke_stays_in_the_dialog_and_says_why", async () => {
  const user = userEvent.setup();
  route(HOSTS, () => [host("nas")]);
  route(REVOKE, () => failure(500, "internal", "The hub could not reach its database."));
  renderAt("/hosts", "/hosts", <HostsPage />);

  await openMenu(user, "nas", "Revoke agent");
  const dialog = screen.getByRole("dialog");
  await user.type(within(dialog).getByLabelText(/Type nas to confirm/), "nas");
  await user.click(within(dialog).getByRole("button", { name: "Revoke agent" }));

  expect(await within(dialog).findByRole("alert")).toHaveTextContent("Could not revoke the agent");
  expect(screen.getByRole("dialog")).toBeVisible();
  expect(screen.queryByText("Agent on nas revoked")).toBeNull();
});

test("a_host_without_an_agent_cannot_be_revoked", async () => {
  const user = userEvent.setup();
  route(HOSTS, () => [host("hub", { kind: "local", agent_enrolled: false, connected: false })]);
  renderAt("/hosts", "/hosts", <HostsPage />);

  await user.click(await screen.findByRole("button", { name: "Actions for hub" }));
  const item = screen.getByRole("menuitem", { name: /Revoke agent/ });
  expect(item).toHaveAttribute("aria-disabled", "true");
  expect(item).toHaveTextContent("No agent is enrolled on this host");
  await user.click(item);
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("hosts_page_shows_a_skeleton_while_loading", async () => {
  route(HOSTS, () => new Promise(() => undefined));
  renderAt("/hosts", "/hosts", <HostsPage />);

  expect(await screen.findByRole("heading", { name: "Hosts" })).toBeVisible();
  expect(screen.getByText("Loading…")).toBeInTheDocument();
  expect(screen.queryByRole("table")).toBeNull();
  expect(screen.queryByText("No hosts yet")).toBeNull();
});

test("a_failed_load_says_so_and_retries", async () => {
  const user = userEvent.setup();
  let fail = true;
  route(HOSTS, () => (fail ? failure(500, "internal", "The hub is busy.") : [host("nas")]));
  renderAt("/hosts", "/hosts", <HostsPage />);

  expect(await screen.findByText("Could not load hosts")).toBeVisible();
  expect(screen.queryByText("No hosts yet"), "a failed load never reads as empty").toBeNull();

  fail = false;
  await user.click(screen.getByRole("button", { name: "Retry" }));
  expect(await screen.findByRole("link", { name: "nas" })).toBeVisible();
});

test("no_hosts_explains_hosts_and_offers_the_first_one", async () => {
  const user = userEvent.setup();
  route(HOSTS, () => []);
  renderAt("/hosts", "/hosts", <HostsPage />);

  expect(await screen.findByText("No hosts yet")).toBeVisible();
  expect(screen.getByText(/runs a small agent that connects to this hub/)).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Add your first host" }));
  expect(screen.getByRole("dialog", { name: "Add a host" })).toBeVisible();
});

test("hosts_page_says_it_shows_all_hosts_while_one_is_selected", async () => {
  route(HOSTS, () => [nas]);
  const selected = renderAt("/hosts?host=nas", "/hosts", <HostsPage />);
  expect(await screen.findByText("Showing all hosts")).toBeVisible();

  selected.unmount();
  localStorage.clear();
  renderAt("/hosts", "/hosts", <HostsPage />);
  await screen.findByRole("link", { name: "nas" });
  expect(screen.queryByText("Showing all hosts")).toBeNull();
});

/** The invite the hub answers with, good for ten minutes from `NOW`. */
const INVITE = {
  host_id: "host-pi",
  token: "tok-1",
  expires_at: "2026-11-01T12:10:00Z",
  fingerprint: "AB:CD:EF:01",
  command: "domovoi agent enroll --hub wss://hub:7421 --token tok-1 --fingerprint AB:CD:EF:01",
};

function timers() {
  vi.useFakeTimers({ now: NOW, shouldAdvanceTime: true });
  return userEvent.setup({ advanceTimers: (ms) => vi.advanceTimersByTime(ms) });
}

/** Names the host `name` in the open dialog and creates its invite. */
async function createInvite(user: ReturnType<typeof userEvent.setup>, name: string) {
  const dialog = screen.getByRole("dialog", { name: "Add a host" });
  await user.type(within(dialog).getByLabelText("Host name"), name);
  await user.click(within(dialog).getByRole("button", { name: "Create invite" }));
  return screen.findByRole("dialog", { name: `Enroll ${name}` });
}

/** Opens the enroll dialog from the page's "Add host" button. */
async function openDialog(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByRole("button", { name: "Add host" }));
}

test("enroll_dialog_shows_the_steps_the_command_and_the_waiting_state_then_success", async () => {
  const user = timers();
  const pi = host("pi", { id: "host-pi", connected: false, version: null });
  let phase: "before" | "invited" | "connected" = "before";
  route(HOSTS, () => [
    nas,
    ...(phase === "before" ? [] : [phase === "invited" ? pi : { ...pi, connected: true }]),
  ]);
  route("POST /api/v1/hosts/{name}/enrollment", () => {
    phase = "invited";
    return INVITE;
  });
  renderAt("/hosts", "/hosts", <HostsPage />);

  await user.click(await screen.findByRole("button", { name: "Add host" }));
  const form = screen.getByRole("dialog", { name: "Add a host" });
  expect(
    within(form)
      .getAllByRole("listitem")
      .map((step) => step.querySelector("p")?.textContent),
  ).toEqual(["Create an invite", "Copy the command", "Wait for the agent"]);
  await user.type(within(form).getByLabelText("Host name"), "pi");
  await user.click(within(form).getByRole("button", { name: "Create invite" }));

  const enroll = await screen.findByRole("dialog", { name: "Enroll pi" });
  expect(
    calls.find((c) => c.route === "POST /api/v1/hosts/{name}/enrollment")?.init.params?.path,
  ).toEqual({ name: "pi" });
  expect(within(enroll).getByLabelText("Enrollment command")).toHaveTextContent(INVITE.command);
  expect(within(enroll).getByLabelText("Hub fingerprint")).toHaveTextContent("AB:CD:EF:01");
  expect(within(enroll).getByText(/The invite expires in/)).toHaveTextContent("10:00");
  expect(within(enroll).getByRole("status")).toHaveTextContent("Waiting for the agent to connect…");

  await act(() => vi.advanceTimersByTimeAsync(2_500));
  expect(within(enroll).getByText(/The invite expires in/)).toHaveTextContent("9:58");
  expect(
    within(enroll).getByRole("status"),
    "a host that was already connected does not count",
  ).toHaveTextContent("Waiting for the agent to connect…");

  phase = "connected";
  await act(() => vi.advanceTimersByTimeAsync(2_000));
  expect(await within(enroll).findByText("pi connected")).toBeVisible();
  expect(within(enroll).queryByText("Waiting for the agent to connect…")).toBeNull();
  expect(within(enroll).getByRole("link", { name: "Open pi" })).toHaveAttribute(
    "href",
    "/hosts/pi",
  );
});

test("enroll_dialog_polls_the_hosts_only_while_it_waits", async () => {
  const user = timers();
  const pi = host("pi", { id: "host-pi", connected: false });
  let connected = false;
  route(HOSTS, () => [nas, connected ? { ...pi, connected: true } : pi]);
  route("POST /api/v1/hosts/{name}/enrollment", () => INVITE);
  renderAt("/hosts", "/hosts", <HostsPage />);

  await openDialog(user);
  const opened = count(HOSTS);
  await act(() => vi.advanceTimersByTimeAsync(6_000));
  expect(count(HOSTS), "no polling before there is an invite").toBe(opened);

  await createInvite(user, "pi");
  const before = count(HOSTS);
  await act(() => vi.advanceTimersByTimeAsync(6_000));
  expect(count(HOSTS), "polls every couple of seconds while waiting").toBeGreaterThan(before + 1);

  connected = true;
  await act(() => vi.advanceTimersByTimeAsync(2_500));
  await screen.findByText("pi connected");
  const settled = count(HOSTS);
  await act(() => vi.advanceTimersByTimeAsync(10_000));
  expect(count(HOSTS), "stops once the agent connected").toBe(settled);
});

test("enroll_dialog_says_when_the_invite_expired", async () => {
  const user = timers();
  route(HOSTS, () => [nas]);
  route("POST /api/v1/hosts/{name}/enrollment", () => ({
    ...INVITE,
    expires_at: "2026-11-01T12:00:05Z",
  }));
  renderAt("/hosts", "/hosts", <HostsPage />);
  await openDialog(user);
  const enroll = await createInvite(user, "pi");

  await act(() => vi.advanceTimersByTimeAsync(6_000));
  expect(within(enroll).getByText("The invite expired")).toBeVisible();
  expect(within(enroll).queryByText("Waiting for the agent to connect…")).toBeNull();
  expect(within(enroll).getByRole("button", { name: "New invite" })).toBeEnabled();
});
