import { act, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { calls, resetApi, route } from "../../test/apiMock";
import { nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { HostsPage } from "./HostsPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const NOW = new Date("2026-11-01T12:00:00Z");

beforeEach(resetApi);

afterEach(() => {
  vi.useRealTimers();
});

function posts(path: string) {
  return calls.filter((c) => c.route === `POST ${path}`);
}

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
