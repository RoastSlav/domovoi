import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, test, vi } from "vitest";
import type { Action, Host, OperationSummary } from "../../api/types";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { OperationsPage } from "./OperationsPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const T = (n: number) => `2026-01-0${n}T00:00:00Z`;

function operation(id: string, updated: string, over: Partial<OperationSummary> = {}) {
  return {
    id,
    kind: "investigate",
    goal: `Goal ${id}`,
    status: "waiting_approval",
    waiting_for: ["approval"],
    cycle: 0,
    incidents: [],
    hosts: [nas.id],
    created_at: updated,
    updated_at: updated,
    ...over,
  } as OperationSummary;
}

function legacy(id: string, updated: string, over: Partial<Action> = {}) {
  return {
    id,
    skill: "restart_container",
    host_id: nas.id,
    incident_id: "inc-9",
    status: "proposed",
    legacy: true,
    created_at: updated,
    updated_at: updated,
    ...over,
  } as unknown as Action;
}

function serve(ops: OperationSummary[], actions: Action[], next: string | null = null) {
  route("GET /api/v1/hosts", () => [nas as Host]);
  route("GET /api/v1/operations", () => ({ items: ops, next_cursor: next, as_of_seq: 1 }));
  route("GET /api/v1/actions", () => ({ items: actions, next_cursor: null, as_of_seq: 1 }));
}

const callsTo = (key: string) => calls.filter((call) => call.route === key);

beforeEach(resetApi);

test("operations_list_shows_operations_and_legacy_actions_with_a_label", async () => {
  serve([operation("o1", T(3))], [legacy("a1", T(2))]);
  renderAt("/operations", "/operations", <OperationsPage />);
  expect(await screen.findByText("Goal o1")).toBeInTheDocument();
  const table = screen.getByRole("table");
  const rows = within(table).getAllByRole("row").slice(1);
  expect(rows[0]).toHaveTextContent("Goal o1");
  expect(rows[1]).toHaveTextContent("restart_container on nas");
  expect(within(rows[1] as HTMLElement).getByText("Legacy")).toBeInTheDocument();
});

test("legacy_row_links_to_its_incident", async () => {
  serve([], [legacy("a1", T(2))]);
  renderAt("/operations", "/operations", <OperationsPage />);
  const link = await screen.findByRole("link", { name: /restart_container on nas/ });
  expect(link).toHaveAttribute("href", "/incidents/inc-9");
});

test("status_chip_asks_the_hub_to_filter_and_is_kept_in_the_url", async () => {
  serve([], []);
  const user = userEvent.setup();
  renderAt("/operations", "/operations", <OperationsPage />);
  await screen.findByText("No operations yet");
  await user.click(screen.getByRole("radio", { name: "Paused" }));
  await screen.findByText("No operations in this state");
  const last = callsTo("GET /api/v1/operations").at(-1);
  expect(JSON.stringify(last?.init)).toContain('"status":"paused"');
  expect(
    callsTo("GET /api/v1/actions").every((c) => !JSON.stringify(c.init).includes("paused")),
  ).toBe(true);
  const actionCallsBefore = callsTo("GET /api/v1/actions").length;
  await user.click(screen.getByRole("radio", { name: "Running" }));
  await screen.findByText("No operations in this state");
  expect(callsTo("GET /api/v1/actions")).toHaveLength(actionCallsBefore);
  await user.click(screen.getByRole("radio", { name: "Waiting for approval" }));
  await screen.findByText("No operations in this state");
  expect(JSON.stringify(callsTo("GET /api/v1/actions").at(-1)?.init)).toContain(
    '"status":"proposed"',
  );
});

test("empty_list_says_what_operations_are_and_links_to_approvals", async () => {
  serve([], []);
  renderAt("/operations", "/operations", <OperationsPage />);
  expect(await screen.findByText("No operations yet")).toBeInTheDocument();
  expect(screen.getByText(/each new incident is investigated/)).toBeInTheDocument();
  expect(screen.getByRole("link", { name: "Go to Approvals" })).toHaveAttribute(
    "href",
    "/approvals",
  );
});

test("a_failed_operations_load_is_a_load_error_with_retry_not_an_empty_state", async () => {
  route("GET /api/v1/hosts", () => [nas as Host]);
  route("GET /api/v1/actions", () => ({ items: [], next_cursor: null, as_of_seq: 1 }));
  route("GET /api/v1/operations", () => {
    return failure(500, "internal", "boom");
  });
  renderAt("/operations", "/operations", <OperationsPage />);
  expect(await screen.findByRole("button", { name: "Retry" })).toBeInTheDocument();
  expect(screen.queryByText("No operations yet")).not.toBeInTheDocument();
});

test("a_failed_classic_actions_load_still_lists_operations_and_says_so", async () => {
  route("GET /api/v1/hosts", () => [nas as Host]);
  route("GET /api/v1/operations", () => ({
    items: [operation("o1", T(3))],
    next_cursor: null,
    as_of_seq: 1,
  }));
  route("GET /api/v1/actions", () => {
    return failure(500, "internal", "boom");
  });
  renderAt("/operations", "/operations", <OperationsPage />);
  expect(await screen.findByText("Goal o1")).toBeInTheDocument();
  expect(await screen.findByText("Could not load classic actions")).toBeInTheDocument();
});

test("load_more_asks_for_the_next_page_of_the_operations_source", async () => {
  serve([operation("o1", T(3))], [], "cursor-2");
  const user = userEvent.setup();
  renderAt("/operations", "/operations", <OperationsPage />);
  await screen.findByText("Goal o1");
  await user.click(screen.getByRole("button", { name: "Load more" }));
  const last = callsTo("GET /api/v1/operations").at(-1);
  expect(JSON.stringify(last?.init)).toContain("cursor-2");
});
