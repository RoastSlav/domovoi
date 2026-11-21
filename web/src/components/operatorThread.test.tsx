import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { renderAt } from "../test/render";
import { OperatorThread, type OperatorTurn } from "./OperatorThread";

const AT = "2026-11-18T10:00:00Z";

function turns(...rest: OperatorTurn[]): OperatorTurn[] {
  return [{ id: "u", at: AT, role: "user", status: "complete", text: "Why?" }, ...rest];
}

function ui(list: OperatorTurn[], extra: Partial<Parameters<typeof OperatorThread>[0]> = {}) {
  return (
    <OperatorThread
      turns={list}
      draft=""
      label="Ask the Operator"
      hint="Read-only."
      onDraftChange={() => undefined}
      onSend={() => undefined}
      {...extra}
    />
  );
}

test("operator_thread_shows_streamed_text_and_replaces_it_with_the_committed_answer", () => {
  const pending: OperatorTurn = { id: "a", at: AT, role: "assistant", status: "pending", text: "" };
  const view = renderAt(
    "/",
    "/",
    ui(turns(pending), { live: { text: "Streaming now ", tools: [] } }),
  );
  expect(screen.getByText(/Streaming now/)).toBeVisible();
  view.rerender(ui(turns({ ...pending, status: "complete", text: "Saved answer." })));
  expect(screen.getByText("Saved answer.")).toBeVisible();
  expect(screen.queryByText(/Streaming now/)).toBeNull();
});

test("retry_and_stop_call_back", async () => {
  const user = userEvent.setup();
  const onRetry = vi.fn();
  const failed: OperatorTurn = {
    id: "a",
    at: AT,
    role: "assistant",
    status: "failed",
    text: "",
    problem: { title: "No model answered." },
    canRetry: true,
  };
  renderAt("/", "/", ui(turns(failed), { onRetry }));
  await user.click(screen.getByRole("button", { name: "Retry" }));
  expect(onRetry).toHaveBeenCalledWith(failed);
});

test("no_model_replaces_the_composer", () => {
  renderAt("/", "/", ui([], { offReason: "Triage is off." }));
  expect(screen.getByText("Triage is off.")).toBeVisible();
  expect(screen.queryByRole("textbox")).toBeNull();
});
