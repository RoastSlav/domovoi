import { act, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { calls, resetApi, route } from "../../test/apiMock";
import { T0, incident, nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import type { AckRule, AckRuleInput, Predicate } from "../../api/types";
import { HostsPage } from "./HostsPage";
import { HostDetailPage } from "./HostDetailPage";
import { HostStatus } from "./HostStatus";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const NOW = new Date("2026-11-01T12:00:00Z");

beforeEach(resetApi);

afterEach(() => {
  vi.useRealTimers();
});

function posts(path: string) {
  return calls.filter((c) => c.route === `POST ${path}`);
}

test("hosts_page_explains_which_hosts_it_knows", async () => {
  route("GET /api/v1/hosts", () => [nas]);
  renderAt("/hosts", "/hosts", <HostsPage />);

  expect(await screen.findByText(/^Machines Domovoi knows about:/)).toBeVisible();
  expect(
    screen.getByText(/hosts running an agent, and hosts that webhooks report on\./),
  ).toBeVisible();
  expect(
    screen.getByRole("link", { name: "Learn more about hosts (opens in a new tab)" }),
  ).toHaveAttribute("href", "https://RoastSlav.github.io/domovoi/concepts/hub-and-agent.html");
});

test("enrollment_flow_shows_command_fingerprint_and_countdown_then_connected", async () => {
  vi.useFakeTimers({ now: NOW, shouldAdvanceTime: true });
  const user = userEvent.setup({ advanceTimers: (ms) => vi.advanceTimersByTime(ms) });
  const pi = {
    ...nas,
    id: "host-pi",
    name: "pi",
    connected: false,
    version: null,
    open_incidents: 0,
  };
  let connected = false;
  route("GET /api/v1/hosts", () => [
    nas,
    connected ? { ...pi, connected: true, version: "0.0.1" } : pi,
  ]);
  route("POST /api/v1/hosts/{name}/enrollment", () => ({
    host_id: "host-pi",
    token: "tok-1",
    expires_at: "2026-11-01T12:10:00Z",
    fingerprint: "AB:CD:EF:01",
    command: "domovoi agent enroll --hub wss://hub:7421 --token tok-1 --fingerprint AB:CD:EF:01",
  }));
  renderAt("/hosts", "/hosts", <HostsPage />);

  await user.click(await screen.findByRole("button", { name: "Add host" }));
  const dialog = screen.getByRole("dialog", { name: "Add a host" });
  await user.type(within(dialog).getByLabelText("Host name"), "pi");
  await user.click(within(dialog).getByRole("button", { name: "Create invite" }));

  const enroll = await screen.findByRole("dialog", { name: "Enroll pi" });
  expect(posts("/api/v1/hosts/{name}/enrollment")[0]?.init.params?.path).toEqual({ name: "pi" });
  expect(within(enroll).getByLabelText("Enrollment command")).toHaveTextContent(
    "domovoi agent enroll --hub wss://hub:7421 --token tok-1 --fingerprint AB:CD:EF:01",
  );
  expect(within(enroll).getByLabelText("Hub fingerprint")).toHaveTextContent("AB:CD:EF:01");
  expect(within(enroll).getByRole("status")).toHaveTextContent(
    "Waiting for the agent… The invite expires in 10:00.",
  );

  await act(() => vi.advanceTimersByTimeAsync(1_000));
  expect(within(enroll).getByRole("status")).toHaveTextContent("expires in 9:59");

  connected = true;
  await act(() => vi.advanceTimersByTimeAsync(2_000));
  expect(await within(enroll).findByText(/Connected\. pi is enrolled/)).toBeVisible();
  expect(within(enroll).getByRole("status")).toHaveTextContent("runs agent 0.0.1");
});

test("enrollment_invite_expires", async () => {
  vi.useFakeTimers({ now: NOW, shouldAdvanceTime: true });
  const user = userEvent.setup({ advanceTimers: (ms) => vi.advanceTimersByTime(ms) });
  route("GET /api/v1/hosts", () => [nas]);
  route("POST /api/v1/hosts/{name}/enrollment", () => ({
    host_id: "host-pi",
    token: "tok-1",
    expires_at: "2026-11-01T12:00:05Z",
    fingerprint: "AB",
    command: "enroll",
  }));
  renderAt("/hosts", "/hosts", <HostsPage />);
  await user.click(await screen.findByRole("button", { name: "Add host" }));
  await user.type(screen.getByLabelText("Host name"), "pi");
  await user.click(screen.getByRole("button", { name: "Create invite" }));
  const enroll = await screen.findByRole("dialog", { name: "Enroll pi" });
  await act(() => vi.advanceTimersByTimeAsync(6_000));
  expect(within(enroll).getByRole("status")).toHaveTextContent("This invite expired");
  expect(within(enroll).getByRole("button", { name: "New invite" })).toBeEnabled();
});

test("revoke_requires_confirmation", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/hosts", () => [nas]);
  route("POST /api/v1/hosts/{name}/revoke", () => ({ revoked: true }));
  renderAt("/hosts", "/hosts", <HostsPage />);

  await user.click(await screen.findByRole("button", { name: "Revoke" }));
  const dialog = screen.getByRole("dialog", { name: "Revoke the agent on nas?" });
  await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(posts("/api/v1/hosts/{name}/revoke")).toHaveLength(0);

  await user.click(screen.getByRole("button", { name: "Revoke" }));
  await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Revoke" }));
  expect(posts("/api/v1/hosts/{name}/revoke")).toEqual([
    { route: "POST /api/v1/hosts/{name}/revoke", init: { params: { path: { name: "nas" } } } },
  ]);
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("outdated_agent_badge_shown", async () => {
  route("GET /api/v1/hosts", () => [
    { ...nas, version: "0.0.0", outdated: true },
    { ...nas, id: "host-pi", name: "pi", version: "0.0.1", outdated: false, open_incidents: 0 },
  ]);
  renderAt("/hosts", "/hosts", <HostsPage />);

  const rows = await screen.findAllByRole("row");
  const old = rows.find((r) => within(r).queryByRole("link", { name: "nas" }));
  const current = rows.find((r) => within(r).queryByRole("link", { name: "pi" }));
  expect(within(old as HTMLElement).getByText("Outdated")).toBeVisible();
  expect(within(old as HTMLElement).getByRole("link", { name: "1" })).toHaveAttribute(
    "href",
    "/incidents?host=nas",
  );
  expect(within(current as HTMLElement).queryByText("Outdated")).toBeNull();
});

test("refused_agent_names_why_it_was_refused", async () => {
  route("GET /api/v1/hosts", () => [
    {
      ...nas,
      connected: false,
      last_seen: null,
      version: null,
      outdated: false,
      refused: "agent 0.0.1 is older than the minimum 0.0.5; update it",
    },
  ]);
  renderAt("/hosts", "/hosts", <HostsPage />);

  const cells = await screen.findAllByRole("cell");
  const refused = cells.filter((cell) => cell.textContent.includes("older than the minimum"));
  expect(refused).toHaveLength(1);
  expect(
    within(refused[0] as HTMLElement).getByText(
      "agent 0.0.1 is older than the minimum 0.0.5; update it",
    ),
  ).toBeVisible();
  expect(within(refused[0] as HTMLElement).queryByText("Outdated")).toBeNull();
});

test("hub_host_without_agent_is_shown_as_hub_only", async () => {
  const hub = {
    ...nas,
    id: "host-hub",
    name: "hub",
    kind: "local",
    connected: false,
    agent_enrolled: false,
    last_seen: null,
  };
  const remote = {
    ...nas,
    id: "host-remote",
    name: "remote",
    kind: "remote",
    connected: false,
    agent_enrolled: false,
    version: null,
    last_seen: null,
  };
  renderAt(
    "/status",
    "/status",
    <>
      <HostStatus host={hub} />
      <HostStatus host={remote} />
    </>,
  );
  expect(screen.getByText("Hub only, no agent")).toHaveAttribute("data-connected", "hub");
  expect(screen.getByText("Never connected")).toHaveAttribute("data-connected", "false");
  expect(screen.queryAllByText("Hub only, no agent")).toHaveLength(1);
  expect(screen.queryAllByText("Never connected")).toHaveLength(1);
});

test("hub_machine_with_a_never_seen_agent_still_shows_an_agent", async () => {
  const hub = {
    ...nas,
    id: "host-hub",
    name: "hub",
    kind: "local",
    connected: false,
    agent_enrolled: true,
    version: null,
    last_seen: null,
  };
  renderAt("/status", "/status", <HostStatus host={hub} />);
  const offline = screen.getByText("Never connected");
  expect(offline).toHaveAttribute("data-connected", "false");
  expect(screen.queryByText("Hub only, no agent")).toBeNull();
});

test("host_detail_page_hints_how_to_cover_the_hub_machine", async () => {
  const hub = {
    ...nas,
    id: "host-hub",
    name: "hub",
    kind: "local",
    connected: false,
    agent_enrolled: false,
    version: null,
    open_incidents: 0,
  };
  route("GET /api/v1/hosts/{name}", () => ({ host: hub, facts: [] }));
  route("GET /api/v1/incidents", () => ({ pages: [] }));
  route("GET /api/v1/hosts/{name}/skills", () => []);
  renderAt("/hosts/hub", "/hosts/:name", <HostDetailPage />);
  const hint = await screen.findByText(/^This is the hub/);
  expect(hint).toHaveTextContent(
    "install an agent on it and enroll it under the name hub. The hub can also run the agent " +
      "in its own process with --with-local-agent /etc/domovoi/agent.toml, where checks that " +
      "need root report errors.",
  );
  expect(within(hint).getByText("--with-local-agent /etc/domovoi/agent.toml")).toBeVisible();
});

test("host_detail_page_has_no_hint_when_an_agent_is_enrolled", async () => {
  const hub = {
    ...nas,
    id: "host-hub",
    name: "hub",
    kind: "local",
    connected: false,
    agent_enrolled: true,
    version: null,
    open_incidents: 0,
  };
  route("GET /api/v1/hosts/{name}", () => ({ host: hub, facts: [] }));
  route("GET /api/v1/incidents", () => ({ pages: [] }));
  route("GET /api/v1/hosts/{name}/skills", () => []);
  renderAt("/hosts/hub", "/hosts/:name", <HostDetailPage />);
  await screen.findByRole("heading", { name: "hub" });
  expect(screen.queryByText(/^This is the hub/)).toBeNull();
  expect(screen.getByText("Never connected")).toHaveAttribute("data-connected", "false");
});

test("recent_incidents_show_resolved_badge_and_triage_note", async () => {
  route("GET /api/v1/hosts/{name}", () => ({ host: nas, facts: [] }));
  route("GET /api/v1/incidents", () => ({
    items: [incident({ status: "resolved", verdict: "ACTION_NEEDED" })],
    next_cursor: null,
  }));
  route("GET /api/v1/hosts/{name}/skills", () => []);
  renderAt("/hosts/nas", "/hosts/:name", <HostDetailPage />);

  const rows = await screen.findAllByRole("listitem");
  const row = rows.find((li) => within(li).queryByRole("link", { name: "Disk /srv is 93% full" }));
  expect(within(row as HTMLElement).getByText("Resolved")).toBeVisible();
  expect(within(row as HTMLElement).getByText(/triage said Action needed/)).toBeVisible();
  expect(screen.queryByText("Action needed")).toBeNull();
});

/** An expected SSH login as the API returns it, overridable. */
function loginRule(
  id: string,
  user: string,
  predicates: Predicate[],
  expires: string | null,
  overrides: Partial<AckRule> = {},
): AckRule {
  return {
    id,
    host: "nas",
    check: "security.ssh_login",
    target_pattern: user,
    predicates,
    note: "Expected SSH login",
    expires_at: expires,
    ...overrides,
  };
}

/** The host page routes, with `rules` as the stored known conditions. */
function loginsRoutes(rules: AckRule[]) {
  route("GET /api/v1/hosts/{name}", () => ({ host: nas, facts: [] }));
  route("GET /api/v1/incidents", () => ({ items: [], next_cursor: null }));
  route("GET /api/v1/hosts/{name}/skills", () => []);
  route("GET /api/v1/ack-rules", () => [...rules]);
  route("POST /api/v1/ack-rules", (init) => {
    const body = init.body as AckRuleInput;
    const created: AckRule = {
      id: "rule-new",
      host: body.host ?? null,
      check: body.check,
      target_pattern: body.target_pattern ?? null,
      predicates: body.predicates,
      note: body.note,
      expires_at:
        body.expires_in_minutes == null
          ? null
          : new Date(NOW.getTime() + body.expires_in_minutes * 60_000).toISOString(),
    };
    rules.push(created);
    return created;
  });
  route("DELETE /api/v1/ack-rules/{id}", (init) => {
    const at = rules.findIndex((rule) => rule.id === init.params?.path?.id);
    rules.splice(at, 1);
    return null;
  });
}

/** Open the SSH logins tab of the host page. */
async function openLogins(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByRole("tab", { name: "SSH logins" }));
}

test("ssh_logins_tab_shows_only_the_expected_logins_of_this_host", async () => {
  vi.useFakeTimers({ now: NOW, shouldAdvanceTime: true });
  const user = userEvent.setup({ advanceTimers: (ms) => vi.advanceTimersByTime(ms) });
  loginsRoutes([
    loginRule(
      "r-1",
      "alice",
      [{ key: "ip", op: "in_network", value: "192.168.1.0/24" }],
      "2026-11-04T12:00:00Z",
    ),
    loginRule("r-2", "bob", [{ key: "network", op: "equals", value: "lan" }], null),
    loginRule("r-3", "carol", [], null, { check: "disk.usage" }),
    loginRule("r-4", "dave", [], null, { host: "pi" }),
    loginRule("r-5", "erin", [], null, { host: null }),
  ]);
  renderAt("/hosts/nas", "/hosts/:name", <HostDetailPage />);
  await openLogins(user);

  const list = screen.getByRole("region", { name: "Expected logins" });
  const items = within(list).getAllByRole("listitem");
  // A rule for any host silences logins on this host too, so it is listed and labelled.
  expect(items.map((item) => item.textContent)).toEqual([
    "alice from the range 192.168.1.0/24, in 3 days Remove",
    "bob from the local network, always Remove",
    "erin from anywhere on any host, always Remove",
  ]);
  expect(within(list).queryByText(/carol/)).toBeNull();
  expect(screen.queryByText(/dave/)).toBeNull();
});

test("login_window_asks_first_and_expects_local_network_logins_for_an_hour", async () => {
  vi.useFakeTimers({ now: NOW, shouldAdvanceTime: true });
  const user = userEvent.setup({ advanceTimers: (ms) => vi.advanceTimersByTime(ms) });
  loginsRoutes([]);
  renderAt("/hosts/nas", "/hosts/:name", <HostDetailPage />);
  await openLogins(user);

  await user.click(screen.getByRole("button", { name: "I’m logging in now" }));
  expect(posts("/api/v1/ack-rules")).toHaveLength(0);

  await user.click(screen.getByRole("button", { name: "Open the window" }));
  expect(posts("/api/v1/ack-rules")[0]?.init.body).toEqual({
    host: "nas",
    check: "security.ssh_login",
    target_pattern: null,
    predicates: [{ key: "network", op: "equals", value: "lan" }],
    note: "Login window opened from the web UI",
    expires_in_minutes: 60,
  });
  const windowSection = screen.getByRole("region", { name: "Logging in now" });
  expect(await within(windowSection).findByRole("status")).toHaveTextContent(
    "A login window is open on nas, so logins from the local network are expected. " +
      "It expires in 1 hour.",
  );
});

test("cancelling_the_login_window_opens_nothing", async () => {
  const user = userEvent.setup();
  loginsRoutes([]);
  renderAt("/hosts/nas", "/hosts/:name", <HostDetailPage />);
  await openLogins(user);

  await user.click(screen.getByRole("button", { name: "I’m logging in now" }));
  await user.click(screen.getByRole("button", { name: "Cancel" }));

  expect(posts("/api/v1/ack-rules")).toHaveLength(0);
  expect(
    within(screen.getByRole("region", { name: "Logging in now" })).queryByRole("status"),
  ).toBeNull();
});

test("login_window_includes_internet_logins_only_when_asked", async () => {
  vi.useFakeTimers({ now: NOW, shouldAdvanceTime: true });
  const user = userEvent.setup({ advanceTimers: (ms) => vi.advanceTimersByTime(ms) });
  loginsRoutes([]);
  renderAt("/hosts/nas", "/hosts/:name", <HostDetailPage />);
  await openLogins(user);

  await user.click(screen.getByRole("button", { name: "I’m logging in now" }));
  await user.click(screen.getByRole("checkbox", { name: "Include logins from the internet" }));
  await user.click(screen.getByRole("button", { name: "Open the window" }));

  expect(posts("/api/v1/ack-rules")[0]?.init.body).toMatchObject({ predicates: [] });
  expect(
    await within(screen.getByRole("region", { name: "Logging in now" })).findByRole("status"),
  ).toHaveTextContent(
    "A login window is open on nas, so any login, including from the internet, is expected. " +
      "It expires in 1 hour.",
  );
});

test("the_login_window_status_shows_after_a_reload", async () => {
  vi.useFakeTimers({ now: NOW, shouldAdvanceTime: true });
  const user = userEvent.setup({ advanceTimers: (ms) => vi.advanceTimersByTime(ms) });
  loginsRoutes([]);
  const { unmount } = renderAt("/hosts/nas", "/hosts/:name", <HostDetailPage />);
  await openLogins(user);
  await user.click(screen.getByRole("button", { name: "I’m logging in now" }));
  await user.click(screen.getByRole("button", { name: "Open the window" }));

  // A reload reads the window from the stored rules, not from local state.
  unmount();
  renderAt("/hosts/nas", "/hosts/:name", <HostDetailPage />);
  await openLogins(user);
  expect(
    await within(screen.getByRole("region", { name: "Logging in now" })).findByRole("status"),
  ).toHaveTextContent(
    "A login window is open on nas, so logins from the local network are expected. " +
      "It expires in 1 hour.",
  );
});

test("the_login_window_status_is_not_shown_after_the_window_expires", async () => {
  vi.useFakeTimers({ now: NOW, shouldAdvanceTime: true });
  const user = userEvent.setup({ advanceTimers: (ms) => vi.advanceTimersByTime(ms) });
  loginsRoutes([
    loginRule(
      "r-1",
      "alice",
      [{ key: "network", op: "equals", value: "lan" }],
      "2026-10-31T13:00:00Z",
      {
        target_pattern: null,
      },
    ),
  ]);
  renderAt("/hosts/nas", "/hosts/:name", <HostDetailPage />);
  await openLogins(user);

  expect(
    within(screen.getByRole("region", { name: "Logging in now" })).queryByRole("status"),
  ).toBeNull();
});

test("expected_login_form_expects_the_user_from_the_local_network", async () => {
  const user = userEvent.setup();
  loginsRoutes([]);
  renderAt("/hosts/nas", "/hosts/:name", <HostDetailPage />);
  await openLogins(user);

  await user.type(screen.getByLabelText("User"), "alice");
  await user.selectOptions(screen.getByLabelText("From"), "lan");
  await user.selectOptions(screen.getByLabelText("For"), "1440");
  await user.click(screen.getByRole("button", { name: "Add expected login" }));

  expect(posts("/api/v1/ack-rules")[0]?.init.body).toEqual({
    host: "nas",
    check: "security.ssh_login",
    target_pattern: "alice",
    predicates: [{ key: "network", op: "equals", value: "lan" }],
    note: "Expected SSH login",
    expires_in_minutes: 1440,
  });
});

test("expected_login_form_expects_an_address_range", async () => {
  const user = userEvent.setup();
  loginsRoutes([]);
  renderAt("/hosts/nas", "/hosts/:name", <HostDetailPage />);
  await openLogins(user);

  await user.type(screen.getByLabelText("User"), "alice");
  await user.selectOptions(screen.getByLabelText("From"), "range");
  await user.type(screen.getByLabelText("Address or range"), "192.168.1.0/24");
  await user.click(screen.getByRole("button", { name: "Add expected login" }));

  expect(posts("/api/v1/ack-rules")[0]?.init.body).toMatchObject({
    target_pattern: "alice",
    predicates: [{ key: "ip", op: "in_network", value: "192.168.1.0/24" }],
    expires_in_minutes: 60,
  });
});

test("expected_login_form_expects_the_single_address_it_is_given", async () => {
  vi.useFakeTimers({ now: NOW, shouldAdvanceTime: true });
  const user = userEvent.setup({ advanceTimers: (ms) => vi.advanceTimersByTime(ms) });
  loginsRoutes([]);
  renderAt("/hosts/nas", "/hosts/:name", <HostDetailPage />);
  await openLogins(user);

  await user.type(screen.getByLabelText("User"), "alice");
  await user.selectOptions(screen.getByLabelText("From"), "range");
  await user.type(screen.getByLabelText("Address or range"), "192.168.1.20");
  await user.click(screen.getByRole("button", { name: "Add expected login" }));

  // One address is sent as typed, and the hub accepts it as that one machine.
  expect(posts("/api/v1/ack-rules")[0]?.init.body).toMatchObject({
    target_pattern: "alice",
    predicates: [{ key: "ip", op: "in_network", value: "192.168.1.20" }],
    expires_in_minutes: 60,
  });
  const list = screen.getByRole("region", { name: "Expected logins" });
  expect(within(list).getAllByRole("listitem")[0]?.textContent).toBe(
    "alice from the address 192.168.1.20, in 1 hour Remove",
  );
});

test("the_address_or_range_is_needed_before_a_login_is_expected", async () => {
  const user = userEvent.setup();
  loginsRoutes([]);
  renderAt("/hosts/nas", "/hosts/:name", <HostDetailPage />);
  await openLogins(user);

  await user.type(screen.getByLabelText("User"), "alice");
  await user.selectOptions(screen.getByLabelText("From"), "range");
  await user.click(screen.getByRole("button", { name: "Add expected login" }));

  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Type the address, or the range in CIDR form, for example 192.168.1.0/24.",
  );
  expect(posts("/api/v1/ack-rules")).toHaveLength(0);
});

test("removing_an_expected_login_asks_then_deletes_its_rule", async () => {
  const user = userEvent.setup();
  loginsRoutes([
    loginRule("r-1", "alice", [{ key: "ip", op: "in_network", value: "192.168.1.0/24" }], null),
  ]);
  renderAt("/hosts/nas", "/hosts/:name", <HostDetailPage />);
  await openLogins(user);

  await user.click(
    screen.getByRole("button", {
      name: "Remove the expected login alice from the range 192.168.1.0/24",
    }),
  );
  expect(calls.filter((c) => c.route === "DELETE /api/v1/ack-rules/{id}")).toHaveLength(0);

  await user.click(screen.getByRole("button", { name: "Remove" }));

  expect(calls.find((c) => c.route === "DELETE /api/v1/ack-rules/{id}")?.init.params?.path).toEqual(
    { id: "r-1" },
  );
  expect(await screen.findByText("No logins are marked as expected on nas.")).toBeVisible();
});

test("cancelling_the_removal_keeps_the_expected_login", async () => {
  const user = userEvent.setup();
  loginsRoutes([
    loginRule("r-1", "alice", [{ key: "ip", op: "in_network", value: "192.168.1.0/24" }], null),
  ]);
  renderAt("/hosts/nas", "/hosts/:name", <HostDetailPage />);
  await openLogins(user);

  await user.click(
    screen.getByRole("button", {
      name: "Remove the expected login alice from the range 192.168.1.0/24",
    }),
  );
  await user.click(screen.getByRole("button", { name: "Cancel" }));

  expect(calls.filter((c) => c.route === "DELETE /api/v1/ack-rules/{id}")).toHaveLength(0);
  expect(screen.getByRole("region", { name: "Expected logins" })).toHaveTextContent(
    "alice from the range 192.168.1.0/24, always",
  );
});

test("host_logs_tab_shows_the_recent_lines_that_agent_kept", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/hosts/{name}", () => ({ host: nas, facts: [] }));
  route("GET /api/v1/incidents", () => ({ items: [], next_cursor: null }));
  route("GET /api/v1/hosts/{name}/skills", () => []);
  route("GET /api/v1/hosts/{name}/logs", () => [
    { at: T0, level: "warn", target: "agent.checks", message: "disk.usage on /srv is 97% full" },
  ]);
  renderAt("/hosts/nas", "/hosts/:name", <HostDetailPage />);
  await user.click(await screen.findByRole("tab", { name: "Logs" }));

  const line = await screen.findByText(/disk\.usage on \/srv is 97% full/);
  expect(line).toHaveAttribute("data-level", "warn");
  expect(calls.filter((c) => c.route === "GET /api/v1/hosts/{name}/logs")[0]?.init.params).toEqual({
    path: { name: "nas" },
    query: { level: "info", lines: 200 },
  });
});

test("outdated_agent_opens_the_update_commands", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/hosts", () => [{ ...nas, version: "0.0.1", outdated: true }]);
  route("GET /api/v1/system/update", () => ({
    current: "0.0.7",
    latest: "v0.9.9",
    newer: true,
    hub_command: "install --role hub --version v0.9.9",
    agent_command: "install --role agent --version v0.9.9",
    docker_command: "docker compose -f hub.yml pull",
  }));
  renderAt("/hosts", "/hosts", <HostsPage />);

  const row = (await screen.findAllByRole("row")).find((r) =>
    within(r).queryByRole("link", { name: "nas" }),
  );
  await user.click(within(row as HTMLElement).getByRole("button", { name: "How to update" }));

  expect(screen.getByRole("dialog", { name: "How to update Domovoi" })).toBeVisible();
  expect(screen.getByLabelText("Agent update command")).toHaveTextContent("--role agent");
});
