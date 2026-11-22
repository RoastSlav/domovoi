import { act, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { applyJournal } from "../../api/journalFrames";
import { failure, resetApi, route } from "../../test/apiMock";
import { summary } from "../../test/attentionFixtures";
import { serveAttention, waitingDetail } from "../../test/attentionRoutes";
import { incident, nas, overview } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { OverviewPage } from "./OverviewPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

beforeEach(() => {
  resetApi();
  localStorage.clear();
  route("GET /api/v1/overview", () => overview());
});

const open = () => renderAt("/", "/", <OverviewPage />);
const region = (name: string) => within(screen.getByRole("region", { name }));
const rowLinks = (name: string, skip: string) =>
  region(name)
    .getAllByRole("link")
    .filter((link) => link.textContent !== skip);

const incidents = (count: number) =>
  Array.from({ length: count }, (_, n) =>
    incident({
      id: `inc-${n}`,
      title: `Problem ${n}`,
      severity: n === 0 ? "critical" : "warning",
      last_firing_at: `2026-10-31T12:0${n}:00Z`,
    }),
  );
const waiting = (n: number) => summary(`op-${n}`, "waiting_approval", { goal: `Plan ${n}` });
const detailsOf = (count: number) =>
  Object.fromEntries(
    Array.from({ length: count }, (_, n) => [`op-${n}`, waitingDetail(`op-${n}`, `Plan ${n}`)]),
  );

test("needs_you_lists_the_first_five_non_approval_items", async () => {
  serveAttention({ incidents: incidents(7), operations: { waiting_approval: [waiting(0)] } });
  open();

  await screen.findByText("Problem 0");
  const links = rowLinks("Needs you", "Attention centre");
  expect(links, "five rows, not seven, and no approval").toHaveLength(5);
  expect(links[0]).toHaveAttribute("href", "/incidents/inc-0");
  expect(links[0]).toHaveTextContent(/Critical.*Problem 0.*nas/);
  expect(region("Needs you").getByRole("link", { name: "Attention centre" })).toHaveAttribute(
    "href",
    "/attention",
  );
});

test("needs_you_orders_by_tier_then_severity_then_recency", async () => {
  const at = (last_firing_at: string) => ({ last_firing_at });
  serveAttention({
    incidents: [
      incident({ id: "i", title: "Info newest", severity: "info", ...at("2026-10-31T12:30:00Z") }),
      incident({
        id: "w1",
        title: "Warning old",
        severity: "warning",
        ...at("2026-10-31T11:00:00Z"),
      }),
      incident({
        id: "c",
        title: "Critical old",
        severity: "critical",
        ...at("2026-10-31T10:00:00Z"),
      }),
      incident({
        id: "w2",
        title: "Warning new",
        severity: "warning",
        ...at("2026-10-31T12:00:00Z"),
      }),
    ],
    operations: {
      paused: [summary("unk", "paused", { pause_reason: { type: "outcome_unknown" } })],
    },
  });
  open();

  await screen.findByText("Critical old");
  const titles = rowLinks("Needs you", "Attention centre").map((link) => link.textContent ?? "");
  expect(titles[0]).toMatch(/Goal unk/);
  expect(titles[1]).toMatch(/Critical old/);
  expect(titles[2]).toMatch(/Warning new/);
  expect(titles[3]).toMatch(/Warning old/);
  expect(titles[4]).toMatch(/Info newest/);
});

test("needs_you_says_nothing_else_needs_you_when_only_approvals_wait", async () => {
  serveAttention({
    hosts: [nas],
    operations: { waiting_approval: [waiting(0)] },
    details: detailsOf(1),
  });
  open();

  expect(await screen.findByText("Nothing else needs you")).toBeVisible();
  expect(screen.queryByText("Nothing needs you")).toBeNull();
});

test("needs_you_never_reads_a_failed_source_as_empty", async () => {
  serveAttention({ hosts: [nas] });
  route("GET /api/v1/incidents", () => failure(500, "internal", "the hub is sick"));
  open();

  expect((await screen.findAllByText("Could not load incidents")).length).toBeGreaterThan(0);
  expect(screen.getAllByRole("button", { name: "Retry" }).length).toBeGreaterThan(0);
  expect(screen.queryByText("Nothing needs you"), "a failure is not an empty list").toBeNull();
});

test("waiting_approvals_are_cards_decided_in_place_with_secondary_approve", async () => {
  serveAttention({
    hosts: [nas],
    operations: { waiting_approval: [0, 1, 2, 3].map(waiting) },
    details: detailsOf(4),
  });
  open();

  await vi.waitFor(() =>
    expect(region("Waiting for approval").getAllByRole("button", { name: "Approve" })).toHaveLength(
      3,
    ),
  );
  const approve = region("Waiting for approval").getAllByRole("button", { name: "Approve" })[0];
  expect(approve?.className, "the quieter button with a check icon").toMatch(/approveIcon/);
  expect(
    region("Waiting for approval").getByRole("link", { name: "1 more waiting" }),
  ).toHaveAttribute("href", "/attention");
});

test("no_step_has_two_approve_buttons_on_the_control_room", async () => {
  serveAttention({
    hosts: [nas],
    operations: { waiting_approval: [waiting(0), waiting(1)] },
    details: detailsOf(2),
  });
  open();

  await vi.waitFor(() =>
    expect(screen.getAllByRole("group", { name: "Action restart_container" })).toHaveLength(2),
  );
  expect(screen.getAllByRole("button", { name: "Approve" }), "one per card").toHaveLength(2);
});

test("running_now_lists_running_operations_with_links", async () => {
  serveAttention({
    hosts: [nas],
    operations: { running: [summary("r1", "running", { goal: "Fix the disk" })] },
  });
  open();

  const link = await region("Running now").findByRole("link", { name: /Fix the disk/ });
  expect(link).toHaveAttribute("href", "/operations/r1");
  expect(link).toHaveTextContent("Running");
  expect(link).toHaveTextContent("nas");
});

test("a_long_host_name_in_a_needs_you_row_is_cut_and_keeps_its_tooltip", async () => {
  const user = userEvent.setup();
  const long = "edge-router-gateway-frankfurt-primary-uplink-monitor-and-failover-probe";
  serveAttention({ hosts: [{ ...nas, name: long }], incidents: [incident()] });
  open();

  await screen.findByText("Disk /srv is 93% full");
  const name = region("Needs you").getByText(long);
  expect(name.className, "one line with an ellipsis").toMatch(/host/);
  await user.hover(name);
  expect(await screen.findByRole("tooltip")).toHaveTextContent(long);
});

test("a_journal_frame_refreshes_the_control_room", async () => {
  serveAttention({ hosts: [nas] });
  const { client } = open();
  await screen.findByText("Nothing is running");

  serveAttention({
    hosts: [nas],
    operations: { running: [summary("r2", "running", { goal: "Late arrival" })] },
  });
  await act(() =>
    applyJournal(client, {
      seq: 7,
      aggregate_type: "operation",
      aggregate_id: "r2",
      kind: "operation_started",
      payload_v: 1,
      payload: null,
      actor: "system",
      at: "2026-01-01T00:00:00Z",
      topics: ["operations"],
    }),
  );

  expect(await screen.findByRole("link", { name: /Late arrival/ })).toBeVisible();
});
