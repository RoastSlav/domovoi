import { act, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { calls, resetApi, route } from "../../test/apiMock";
import { incident, nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
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

test("hosts_page_explains_that_each_host_runs_an_agent", async () => {
  route("GET /api/v1/hosts", () => [nas]);
  renderAt("/hosts", "/hosts", <HostsPage />);

  expect(await screen.findByText(/^Machines Domovoi watches\./)).toBeVisible();
  expect(screen.getByRole("link", { name: "Learn more" })).toHaveAttribute(
    "href",
    "https://RoastSlav.github.io/domovoi/concepts/hub-and-agent.html",
  );
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
