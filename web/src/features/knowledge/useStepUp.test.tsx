import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useAcceptFact } from "../../api/hooks/knowledgeWrites";
import { keys } from "../../api/queryClient";
import { useActionFeedback } from "../../components";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { knowledgeFact } from "../../test/knowledgeFixtures";
import { renderAt } from "../../test/render";
import { useStepUp } from "./useStepUp";

vi.mock("../../api/client", () => import("../../test/apiMock"));

interface Vars {
  id: string;
  expectedVersion: number;
}

function Probe({ needsPassword }: { needsPassword: boolean }) {
  const accept = useAcceptFact();
  const feedback = useActionFeedback(accept, {
    success: "Saved.",
    error: "save the fact",
    errorDisplay: "inline",
  });
  const stepUp = useStepUp<Vars>({
    confirmLabel: "Accept",
    reason: () => "Enter your password to save it.",
    run: (variables, password, callbacks) =>
      feedback.mutate({ ...variables, ...(password === undefined ? {} : { password }) }, callbacks),
    busy: feedback.isPending,
    error: feedback.error,
    reset: feedback.reset,
  });
  return (
    <>
      <button onClick={() => stepUp.start({ id: "f1", expectedVersion: 3 }, needsPassword)}>
        Go
      </button>
      {stepUp.inline ? (
        <p>{`inline: ${stepUp.inline.title} ${stepUp.inline.detail ?? ""}`}</p>
      ) : null}
      {stepUp.dialog}
    </>
  );
}

const sent = () => calls.filter((c) => c.route.endsWith("/accept"));

beforeEach(() => {
  resetApi();
  route("POST /api/v1/knowledge/facts/{id}/accept", () => knowledgeFact());
});

describe("useStepUp", () => {
  it("step_up_runs_at_once_when_no_password_is_needed", async () => {
    renderAt("/", "/", <Probe needsPassword={false} />);
    await userEvent.click(screen.getByRole("button", { name: "Go" }));
    await waitFor(() => expect(sent()).toHaveLength(1));
    expect(sent()[0]?.init.body).toEqual({ expected_version: 3 });
    expect(screen.queryByText("Confirm with your password")).toBeNull();
  });

  it("step_up_asks_for_the_password_first_and_sends_it_once", async () => {
    const { client } = renderAt("/", "/", <Probe needsPassword />);
    await userEvent.click(screen.getByRole("button", { name: "Go" }));
    expect(screen.getByText("Enter your password to save it.")).toBeInTheDocument();
    expect(sent()).toHaveLength(0);
    await userEvent.type(screen.getByLabelText("Password"), "s3cret-pw");
    await userEvent.click(screen.getByRole("button", { name: "Accept" }));
    await waitFor(() => expect(sent()).toHaveLength(1));
    expect(sent()[0]?.init.body).toEqual({ expected_version: 3, password: "s3cret-pw" });
    await waitFor(() => expect(screen.queryByLabelText("Password")).toBeNull());
    // The password is in the one request body and nowhere else.
    const elsewhere = JSON.stringify([
      client
        .getQueryCache()
        .getAll()
        .map((q) => q.queryKey),
      calls.map((c) => [c.route, c.init.params]),
      Object.entries(localStorage),
      Object.entries(sessionStorage),
      window.location.href,
      keys.knowledge,
    ]);
    expect(elsewhere).not.toContain("s3cret-pw");
  });

  it("a_wrong_password_reads_that_password_is_not_right_and_keeps_the_prompt", async () => {
    route("POST /api/v1/knowledge/facts/{id}/accept", () =>
      failure(403, "forbidden", "that password is wrong"),
    );
    renderAt("/", "/", <Probe needsPassword />);
    await userEvent.click(screen.getByRole("button", { name: "Go" }));
    await userEvent.type(screen.getByLabelText("Password"), "nope");
    await userEvent.click(screen.getByRole("button", { name: "Accept" }));
    expect(await screen.findByText("That password is not right.")).toBeInTheDocument();
    expect(screen.getByLabelText("Password")).toBeInTheDocument();
  });

  it("a_locked_out_prompt_shows_the_wait", async () => {
    route("POST /api/v1/knowledge/facts/{id}/accept", () =>
      failure(429, "too_many_requests", "slow down", { "retry-after": "28" }),
    );
    renderAt("/", "/", <Probe needsPassword />);
    await userEvent.click(screen.getByRole("button", { name: "Go" }));
    await userEvent.type(screen.getByLabelText("Password"), "x");
    await userEvent.click(screen.getByRole("button", { name: "Accept" }));
    expect(await screen.findByText(/Too many attempts\. Try again in 28 s\./)).toBeInTheDocument();
    expect(screen.queryByText("That password is not right.")).toBeNull();
  });

  it("cancel_closes_the_prompt_and_sends_nothing", async () => {
    renderAt("/", "/", <Probe needsPassword />);
    await userEvent.click(screen.getByRole("button", { name: "Go" }));
    await userEvent.type(screen.getByLabelText("Password"), "x");
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByLabelText("Password")).toBeNull());
    expect(sent()).toHaveLength(0);
  });

  it("the_password_is_not_kept_after_the_call", async () => {
    renderAt("/", "/", <Probe needsPassword />);
    // After a success.
    await userEvent.click(screen.getByRole("button", { name: "Go" }));
    await userEvent.type(screen.getByLabelText("Password"), "first");
    await userEvent.click(screen.getByRole("button", { name: "Accept" }));
    await waitFor(() => expect(screen.queryByLabelText("Password")).toBeNull());
    await userEvent.click(screen.getByRole("button", { name: "Go" }));
    expect(screen.getByLabelText("Password")).toHaveValue("");
    // After a cancel.
    await userEvent.type(screen.getByLabelText("Password"), "second");
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByLabelText("Password")).toBeNull());
    await userEvent.click(screen.getByRole("button", { name: "Go" }));
    expect(screen.getByLabelText("Password")).toHaveValue("");
  });
});
