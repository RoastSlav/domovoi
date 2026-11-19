import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, test, vi } from "vitest";
import { applyJournal } from "../../api/journalFrames";
import type { OperationDetail } from "../../api/types";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { operationDetail } from "../../test/operationFixtures";
import { showOperation } from "../../test/operationPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

beforeEach(resetApi);

const GET = "GET /api/v1/operations/{id}";
const callsTo = (key: string) => calls.filter((call) => call.route === key);

function withStatus(detail: OperationDetail, status: string): OperationDetail {
  return { ...detail, operation: { ...detail.operation, status } };
}

test("no_such_operation_says_so_with_a_link_back", async () => {
  showOperation(operationDetail("verified-success"), {
    get: () => failure(404, "not_found", "no such operation"),
  });
  expect(await screen.findByText("No such operation")).toBeInTheDocument();
  expect(screen.getByRole("link", { name: "Back to Operations" })).toHaveAttribute(
    "href",
    "/operations",
  );
});

test("failed_load_is_a_load_error_with_retry", async () => {
  showOperation(operationDetail("verified-success"), {
    get: () => failure(503, "unavailable", "orchestration is not enabled"),
  });
  await userEvent.setup().click(await screen.findByRole("button", { name: "Retry" }));
  expect(callsTo(GET).length).toBeGreaterThan(1);
});

test("a_failed_refresh_keeps_the_page_and_says_so", async () => {
  const detail = operationDetail("verified-success");
  const { client } = showOperation(detail);
  await screen.findByRole("heading", { level: 1, name: detail.operation.goal });
  route(GET, () => failure(503, "unavailable", "down"));
  await act(() => client.invalidateQueries({ queryKey: ["operations"] }));
  expect(await screen.findByText("Could not refresh this page")).toBeInTheDocument();
  expect(screen.getByRole("heading", { level: 1, name: detail.operation.goal })).toBeVisible();
  expect(screen.getByRole("button", { name: "Retry" })).toBeVisible();
});

test("a_journal_frame_newer_than_the_page_refreshes_it", async () => {
  const detail = operationDetail("verified-success");
  const { client } = showOperation(detail);
  await screen.findByRole("heading", { level: 1, name: detail.operation.goal });
  const before = callsTo(GET).length;
  route(GET, () => ({
    ...withStatus(detail, "failed"),
    as_of_seq: detail.as_of_seq + 1,
  }));
  await act(() =>
    applyJournal(client, {
      seq: detail.as_of_seq + 1,
      aggregate_type: "operation",
      aggregate_id: detail.operation.id,
      kind: "operation_step_finished",
      payload_v: 1,
      payload: null,
      actor: "system",
      at: "2026-01-01T00:00:00Z",
      topics: ["operations"],
    }),
  );
  expect(await screen.findByText("The operation failed")).toBeInTheDocument();
  expect(callsTo(GET).length).toBeGreaterThan(before);
});

test("a_journal_frame_the_page_already_contains_changes_nothing", async () => {
  const detail = operationDetail("verified-success");
  const { client } = showOperation(detail);
  await screen.findByRole("heading", { level: 1, name: detail.operation.goal });
  const before = callsTo(GET).length;
  await act(() =>
    applyJournal(client, {
      seq: detail.as_of_seq,
      aggregate_type: "operation",
      aggregate_id: detail.operation.id,
      kind: "operation_step_finished",
      payload_v: 1,
      payload: null,
      actor: "system",
      at: "2026-01-01T00:00:00Z",
      topics: ["operations"],
    }),
  );
  expect(callsTo(GET)).toHaveLength(before);
});

test("waiting_for_you_lists_each_pending_approval_once_and_the_inspector_card_is_read_only", async () => {
  const detail = operationDetail("waiting-approval");
  showOperation(detail);
  await screen.findByRole("heading", { level: 1, name: detail.operation.goal });
  expect(screen.getAllByRole("button", { name: "Approve" })).toHaveLength(1);
  const step = detail.executions[0]?.step ?? "";
  const row = document.querySelector(`a[href$="?step=${step}"]`) as HTMLElement;
  await userEvent.setup().click(row);
  const inspector = await screen.findByRole("complementary", { name: "Step inspector" });
  expect(within(inspector).getByRole("group")).toBeVisible();
  expect(within(inspector).queryByRole("button", { name: "Approve" })).toBeNull();
  expect(screen.getAllByRole("button", { name: "Approve" })).toHaveLength(1);
});

test("pause_posts_toasts_and_is_hidden_when_finished_or_paused", async () => {
  const detail = withStatus(operationDetail("verified-success"), "running");
  showOperation(detail);
  route("POST /api/v1/operations/{id}/pause", () => ({
    as_of_seq: 9,
    operation: detail.operation,
  }));
  await userEvent.setup().click(await screen.findByRole("button", { name: "Pause" }));
  expect(await screen.findByText(/Operation paused/)).toBeInTheDocument();
  expect(callsTo("POST /api/v1/operations/{id}/pause")).toHaveLength(1);
});

test("pause_is_hidden_when_the_operation_is_finished", async () => {
  const detail = operationDetail("verified-success");
  showOperation(detail);
  await screen.findByRole("heading", { level: 1, name: detail.operation.goal });
  expect(screen.queryByRole("button", { name: "Pause" })).toBeNull();
  expect(screen.queryByRole("button", { name: "More" })).toBeNull();
});

test("pause_of_a_queued_operation_is_disabled_with_a_reason", async () => {
  showOperation(withStatus(operationDetail("verified-success"), "queued"));
  const pause = await screen.findByRole("button", { name: "Pause" });
  expect(pause).toHaveAttribute("aria-disabled", "true");
  await userEvent.setup().hover(pause);
  expect(await screen.findByText("It has not started yet. Cancel it instead.")).toBeVisible();
});

test("cancel_asks_first_names_the_operation_and_sends_the_reason", async () => {
  const detail = withStatus(operationDetail("verified-success"), "running");
  showOperation(detail);
  route("POST /api/v1/operations/{id}/cancel", () => ({
    as_of_seq: 9,
    operation: detail.operation,
  }));
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "More" }));
  await user.click(screen.getByRole("menuitem", { name: /Cancel operation/ }));
  const dialog = await screen.findByRole("dialog", { name: "Cancel this operation?" });
  expect(dialog).toHaveTextContent(detail.operation.goal);
  expect(callsTo("POST /api/v1/operations/{id}/cancel")).toHaveLength(0);
  await user.type(within(dialog).getByRole("textbox"), "not needed");
  await user.click(within(dialog).getByRole("button", { name: "Cancel operation" }));
  await waitFor(() => expect(callsTo("POST /api/v1/operations/{id}/cancel")).toHaveLength(1));
  expect(callsTo("POST /api/v1/operations/{id}/cancel")[0]?.init.body).toEqual({
    reason: "not needed",
  });
});

test("cancel_without_a_reason_sends_an_empty_body_and_a_long_reason_is_blocked", async () => {
  const detail = withStatus(operationDetail("verified-success"), "running");
  showOperation(detail);
  route("POST /api/v1/operations/{id}/cancel", () => ({
    as_of_seq: 9,
    operation: detail.operation,
  }));
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "More" }));
  await user.click(screen.getByRole("menuitem", { name: /Cancel operation/ }));
  const dialog = await screen.findByRole("dialog");
  const confirm = within(dialog).getByRole("button", { name: "Cancel operation" });
  await user.click(within(dialog).getByRole("textbox"));
  await user.paste("x".repeat(501));
  expect(confirm).toHaveAttribute("aria-disabled", "true");
  await user.clear(within(dialog).getByRole("textbox"));
  await user.click(confirm);
  await waitFor(() => expect(callsTo("POST /api/v1/operations/{id}/cancel")).toHaveLength(1));
  expect(callsTo("POST /api/v1/operations/{id}/cancel")[0]?.init.body).toEqual({});
});

test("outcome_unknown_resume_asks_first_and_posts_an_empty_extend", async () => {
  const detail = operationDetail("outcome-unknown-paused");
  showOperation(detail);
  route("POST /api/v1/operations/{id}/resume", () => ({
    as_of_seq: 9,
    operation: detail.operation,
  }));
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "Resume operation" }));
  const dialog = await screen.findByRole("dialog", {
    name: "Continue without knowing the result?",
  });
  expect(callsTo("POST /api/v1/operations/{id}/resume")).toHaveLength(0);
  await user.click(within(dialog).getByRole("button", { name: "Resume operation" }));
  await waitFor(() => expect(callsTo("POST /api/v1/operations/{id}/resume")).toHaveLength(1));
  expect(callsTo("POST /api/v1/operations/{id}/resume")[0]?.init.body).toEqual({ extend: [] });
});

test("manual_pause_resumes_without_a_dialog", async () => {
  const base = operationDetail("verified-success");
  const detail = {
    ...base,
    operation: { ...base.operation, status: "paused", pause_reason: { type: "manual" } },
  } as OperationDetail;
  showOperation(detail);
  route("POST /api/v1/operations/{id}/resume", () => ({
    as_of_seq: 9,
    operation: detail.operation,
  }));
  await userEvent.setup().click(await screen.findByRole("button", { name: "Resume operation" }));
  expect(await screen.findByText("Operation resumed")).toBeInTheDocument();
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("raise_limit_posts_extend_for_the_exhausted_kind_in_api_units", async () => {
  const base = operationDetail("question-and-budget-pause");
  const detail = {
    ...base,
    operation: { ...base.operation, pause_reason: { type: "budget", exhausted: "wall_time" } },
    trace: {
      ...base.trace,
      operation: {
        ...base.trace.operation,
        budgets: {
          limits: { ...base.trace.operation.budgets.limits, wall_seconds: 600 },
          used: { ...base.trace.operation.budgets.used, wall_seconds: 600 },
        },
      },
    },
  } as OperationDetail;
  showOperation(detail);
  route("POST /api/v1/operations/{id}/resume", () => ({
    as_of_seq: 9,
    operation: detail.operation,
  }));
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "Raise limit and resume" }));
  const dialog = await screen.findByRole("dialog", { name: "Raise the Time limit" });
  const input = within(dialog).getByRole("spinbutton");
  await user.clear(input);
  await user.type(input, "20");
  await user.click(within(dialog).getByRole("button", { name: "Raise limit and resume" }));
  await waitFor(() => expect(callsTo("POST /api/v1/operations/{id}/resume")).toHaveLength(1));
  expect(callsTo("POST /api/v1/operations/{id}/resume")[0]?.init.body).toEqual({
    extend: [{ kind: "wall_time", limit: 1200 }],
  });
});

test("raise_limit_below_the_current_limit_is_a_field_error_and_sends_nothing", async () => {
  showOperation(operationDetail("question-and-budget-pause"));
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "Raise limit and resume" }));
  const dialog = await screen.findByRole("dialog");
  const input = within(dialog).getByRole("spinbutton");
  await user.clear(input);
  await user.type(input, "1");
  await user.click(within(dialog).getByRole("button", { name: "Raise limit and resume" }));
  expect(within(dialog).getAllByText(/Enter a limit above/).length).toBeGreaterThan(0);
  expect(callsTo("POST /api/v1/operations/{id}/resume")).toHaveLength(0);
});

test("a_budget_resume_conflict_shows_the_hubs_text_inside_the_dialog", async () => {
  showOperation(operationDetail("question-and-budget-pause"));
  route("POST /api/v1/operations/{id}/resume", () =>
    failure(409, "conflict", "the limit is still spent; raise it first"),
  );
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "Raise limit and resume" }));
  const dialog = await screen.findByRole("dialog");
  await user.click(within(dialog).getByRole("button", { name: "Raise limit and resume" }));
  expect(await within(dialog).findByText(/still spent/)).toBeVisible();
});

test("review_button_moves_focus_to_waiting_for_you", async () => {
  showOperation(operationDetail("waiting-approval"));
  await userEvent.setup().click(await screen.findByRole("button", { name: "Review" }));
  expect(screen.getByRole("heading", { name: "Waiting for you" })).toHaveFocus();
});

test("waiting_agent_banner_names_the_hosts", async () => {
  const base = operationDetail("verified-success");
  showOperation({
    ...base,
    operation: { ...base.operation, status: "waiting_agent", hosts: ["host-nas"] },
  });
  expect(await screen.findByText(/^Waiting for (nas|host-nas)$/)).toBeInTheDocument();
});

test("detail_shows_the_all_hosts_note_only_with_a_host_selected_and_names_the_page", async () => {
  const detail = operationDetail("verified-success");
  showOperation(detail);
  expect(
    await screen.findByRole("heading", { level: 1, name: detail.operation.goal }),
  ).toBeVisible();
});
