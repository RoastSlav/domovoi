import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, test, vi } from "vitest";
import type { Host, OperationSummary } from "../../api/types";
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

function serve(ops: OperationSummary[], next: string | null = null) {
  route("GET /api/v1/hosts", () => [nas as Host]);
  route("GET /api/v1/operations", () => ({ items: ops, next_cursor: next, as_of_seq: 1 }));
}

const callsTo = (key: string) => calls.filter((call) => call.route === key);

beforeEach(resetApi);

test("operations_list_shows_operations_newest_first_with_host_names", async () => {
  serve([operation("o1", T(3)), operation("o2", T(2))]);
  renderAt("/operations", "/operations", <OperationsPage />);
  expect(await screen.findByText("Goal o1")).toBeInTheDocument();
  const rows = screen.getAllByRole("row").slice(1);
  expect(rows[0]).toHaveTextContent("Goal o1");
  expect(rows[0]).toHaveTextContent("nas");
  expect(rows[1]).toHaveTextContent("Goal o2");
});

test("status_chip_asks_the_hub_to_filter_and_is_kept_in_the_url", async () => {
  serve([]);
  const user = userEvent.setup();
  renderAt("/operations", "/operations", <OperationsPage />);
  await screen.findByText("No operations yet");
  await user.click(screen.getByRole("radio", { name: "Paused" }));
  await screen.findByText("No operations in this state");
  const last = callsTo("GET /api/v1/operations").at(-1);
  expect(JSON.stringify(last?.init)).toContain('"status":"paused"');
});

test("empty_list_says_what_operations_are_and_links_to_approvals", async () => {
  serve([]);
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
  route("GET /api/v1/operations", () => {
    return failure(500, "internal", "boom");
  });
  renderAt("/operations", "/operations", <OperationsPage />);
  expect(await screen.findByRole("button", { name: "Retry" })).toBeInTheDocument();
  expect(screen.queryByText("No operations yet")).not.toBeInTheDocument();
});

test("load_more_asks_for_the_next_page_of_the_operations_source", async () => {
  serve([operation("o1", T(3))], "cursor-2");
  const user = userEvent.setup();
  renderAt("/operations", "/operations", <OperationsPage />);
  await screen.findByText("Goal o1");
  await user.click(screen.getByRole("button", { name: "Load more" }));
  const last = callsTo("GET /api/v1/operations").at(-1);
  expect(JSON.stringify(last?.init)).toContain("cursor-2");
});
