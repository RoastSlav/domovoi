import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, test, vi } from "vitest";
import type { OperationDetail } from "../../api/types";
import { at } from "../../test/at";
import { calls, resetApi, route } from "../../test/apiMock";
import { operationDetail } from "../../test/operationFixtures";
import { showOperation } from "../../test/operationPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

beforeEach(resetApi);

const ANSWER = "POST /api/v1/operations/{id}/steps/{step}/answer";

function asking(status: string): OperationDetail {
  const base = operationDetail("verified-success");
  const question = {
    ...at(base.trace.steps, 0),
    id: "step-question",
    kind: "question",
    outcome: "pending",
    summary: "Which container?",
  };
  return {
    ...base,
    operation: { ...base.operation, status },
    trace: { ...base.trace, steps: [...base.trace.steps, question] },
    executions: [],
  } as OperationDetail;
}

test("answer_posts_the_text_clears_the_field_and_toasts", async () => {
  const detail = asking("waiting_user");
  showOperation(detail);
  route(ANSWER, () => ({ as_of_seq: 9, operation: detail.operation }));
  const user = userEvent.setup();
  const send = await screen.findByRole("button", { name: "Send answer" });
  expect(send).toHaveAttribute("aria-disabled", "true");
  const field = screen.getByRole("textbox", { name: "Your answer" });
  await user.type(field, "plex");
  await user.click(send);
  expect(await screen.findByText("Answer sent.")).toBeInTheDocument();
  expect(calls.filter((call) => call.route === ANSWER)[0]?.init.body).toEqual({ text: "plex" });
  expect(field).toHaveValue("");
});

test("answer_is_disabled_with_a_reason_while_the_operation_is_paused", async () => {
  showOperation(asking("paused"));
  const send = await screen.findByRole("button", { name: "Send answer" });
  await userEvent.setup().type(screen.getByRole("textbox", { name: "Your answer" }), "plex");
  expect(send).toHaveAttribute("aria-disabled", "true");
  await userEvent.setup().hover(send);
  expect(await screen.findByText("Resume the operation before answering.")).toBeVisible();
});

test("the_question_text_is_shown_as_plain_text", async () => {
  showOperation(asking("waiting_user"));
  route("GET /api/v1/operations/{id}/steps/{step}", () => ({
    id: "step-question",
    intent: { text: "<b>Which</b> container?" },
  }));
  expect(await screen.findByText("<b>Which</b> container?")).toBeVisible();
});
