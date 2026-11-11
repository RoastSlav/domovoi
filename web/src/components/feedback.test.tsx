import { act, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { ApiFailure } from "../api/types";
import { describeError } from "../design/messages";
import { TOAST_MS, ToastProvider, useToast } from "./Toast";
import { useActionFeedback, type FeedbackMessages } from "./useActionFeedback";

function Raise({ undo }: { undo: () => void }) {
  const toast = useToast();
  return (
    <>
      <button type="button" onClick={() => toast.success("Incident acknowledged")}>
        success
      </button>
      <button
        type="button"
        onClick={() =>
          toast.success({ title: "Incident muted", action: { label: "Undo", onClick: undo } })
        }
      >
        undoable
      </button>
      <button
        type="button"
        onClick={() =>
          toast.error(
            describeError(
              new ApiFailure(409, "conflict", "the approval window closed"),
              "approve the action",
            ),
          )
        }
      >
        failure
      </button>
    </>
  );
}

function setup(undo = vi.fn()) {
  render(
    <ToastProvider>
      <Raise undo={undo} />
    </ToastProvider>,
  );
  return { undo, region: screen.getByRole("log", { name: "Notifications" }) };
}

describe("toasts", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  test("a_success_toast_is_announced_and_leaves_by_itself", () => {
    const { region } = setup();
    expect(region, "one polite live region").toHaveAttribute("aria-live", "polite");
    fireEvent.click(screen.getByRole("button", { name: "success" }));
    expect(within(region).getByText("Incident acknowledged")).toBeVisible();

    act(() => void vi.advanceTimersByTime(TOAST_MS - 100));
    expect(within(region).getByText("Incident acknowledged")).toBeVisible();
    act(() => void vi.advanceTimersByTime(200));
    expect(within(region).queryByText("Incident acknowledged")).toBeNull();
  });

  test("a_toast_stays_while_the_pointer_or_focus_is_on_it", () => {
    const { region } = setup();
    fireEvent.click(screen.getByRole("button", { name: "success" }));
    const toast = within(region).getByText("Incident acknowledged").closest("[data-tone]");
    fireEvent.pointerEnter(toast as Element);
    act(() => void vi.advanceTimersByTime(TOAST_MS * 3));
    expect(within(region).getByText("Incident acknowledged"), "paused on hover").toBeVisible();

    fireEvent.pointerLeave(toast as Element);
    act(() => void vi.advanceTimersByTime(TOAST_MS + 100));
    expect(within(region).queryByText("Incident acknowledged")).toBeNull();
  });

  test("an_error_toast_interrupts_and_stays_until_dismissed", () => {
    const { region } = setup();
    fireEvent.click(screen.getByRole("button", { name: "failure" }));
    const alert = within(region).getByRole("alert");
    expect(alert).toHaveTextContent("Could not approve the action");
    expect(alert).toHaveTextContent("The approval window closed.");

    act(() => void vi.advanceTimersByTime(TOAST_MS * 5));
    expect(within(region).getByRole("alert")).toBeVisible();
    fireEvent.click(within(alert).getByRole("button", { name: "Dismiss" }));
    expect(within(region).queryByRole("alert")).toBeNull();
  });

  test("undo_calls_its_handler_and_closes_the_toast", () => {
    const { region, undo } = setup();
    fireEvent.click(screen.getByRole("button", { name: "undoable" }));
    fireEvent.click(within(region).getByRole("button", { name: "Undo" }));
    expect(undo).toHaveBeenCalledOnce();
    expect(within(region).queryByText("Incident muted")).toBeNull();
  });

  test("escape_dismisses_the_focused_toast_and_only_three_show", () => {
    const { region } = setup();
    for (let i = 0; i < 5; i += 1) {
      fireEvent.click(screen.getByRole("button", { name: "failure" }));
    }
    expect(within(region).getAllByRole("alert"), "at most three").toHaveLength(3);
    const first = within(region).getAllByRole("alert")[0] as HTMLElement;
    fireEvent.keyDown(within(first).getByRole("button", { name: "Dismiss" }), { key: "Escape" });
    expect(within(region).getAllByRole("alert")).toHaveLength(2);
  });
});

function Acting({
  run,
  messages,
}: {
  messages: Partial<FeedbackMessages<string, string>>;
  run: (name: string) => Promise<string>;
}) {
  const [pending, setPending] = useState(false);
  const action = useActionFeedback(
    {
      isPending: pending,
      reset: () => undefined,
      mutateAsync: (name: string) => {
        setPending(true);
        return run(name).finally(() => setPending(false));
      },
    },
    { success: "Incident acknowledged", error: "acknowledge the incident", ...messages },
  );
  return (
    <>
      <button type="button" onClick={() => action.mutate("inc-1")}>
        go
      </button>
      {action.error && <p data-testid="inline">{action.error.title}</p>}
    </>
  );
}

function act_(run: (name: string) => Promise<string>, messages = {}) {
  render(
    <ToastProvider>
      <Acting run={run} messages={messages} />
    </ToastProvider>,
  );
  return screen.getByRole("log", { name: "Notifications" });
}

describe("useActionFeedback", () => {
  const conflict = () => Promise.reject(new ApiFailure(409, "conflict", "it is already resolved"));

  test("a_success_raises_the_success_toast", async () => {
    const user = userEvent.setup();
    const region = act_(() => Promise.resolve("ok"));
    await user.click(screen.getByRole("button", { name: "go" }));
    expect(await within(region).findByText("Incident acknowledged")).toBeVisible();
  });

  test("a_failure_raises_an_error_toast_in_the_catalogue_s_words", async () => {
    const user = userEvent.setup();
    const region = act_(conflict);
    await user.click(screen.getByRole("button", { name: "go" }));
    const alert = await within(region).findByRole("alert");
    expect(alert).toHaveTextContent("Could not acknowledge the incident");
    expect(alert).toHaveTextContent("It is already resolved.");
    expect(alert).not.toHaveTextContent("conflict");
  });

  test("an_inline_failure_raises_no_toast_and_hands_back_the_error", async () => {
    const user = userEvent.setup();
    const region = act_(conflict, { errorDisplay: "inline" });
    await user.click(screen.getByRole("button", { name: "go" }));
    expect(await screen.findByTestId("inline")).toHaveTextContent(
      "Could not acknowledge the incident",
    );
    expect(within(region).queryByRole("alert")).toBeNull();
  });

  test("undo_gets_the_result_and_what_was_sent", async () => {
    const user = userEvent.setup();
    const undo = vi.fn();
    const region = act_(() => Promise.resolve("muted"), { undo });
    await user.click(screen.getByRole("button", { name: "go" }));
    await user.click(await within(region).findByRole("button", { name: "Undo" }));
    expect(undo).toHaveBeenCalledWith("muted", "inc-1");
  });

  test("a_null_success_says_nothing_and_an_info_one_is_not_a_success", async () => {
    const user = userEvent.setup();
    const region = act_(() => Promise.resolve("same"), {
      success: (result: string) =>
        result === "same" ? { title: "Nothing changed", tone: "info" as const } : null,
    });
    await user.click(screen.getByRole("button", { name: "go" }));
    const toast = (await within(region).findByText("Nothing changed")).closest("[data-tone]");
    expect(toast).toHaveAttribute("data-tone", "info");
  });
});
