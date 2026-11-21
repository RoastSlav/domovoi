import { act, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { failure, resetApi, route } from "../../test/apiMock";
import {
  ANSWER,
  CONV,
  TURN,
  conversation,
  failedConversation,
  globalConversation,
  message,
  pendingConversation,
} from "../../test/conversationFixtures";
import { FakeEventSource } from "../../test/fakeEventSource";
import { renderAt } from "../../test/render";
import { OperatorConversation } from "./OperatorConversation";
import { made, routeOperator, sentPair } from "./operatorTestKit";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const SEND = "POST /api/v1/conversations/{id}/messages";
const CANCEL = "POST /api/v1/conversations/{id}/turns/{turn}/cancel";
const RETRY = "POST /api/v1/conversations/{id}/messages/{message}/retry";
const OTHER_TURN = "0190a0a0-0000-7000-8000-0000000000d9";

beforeEach(() => {
  resetApi();
  FakeEventSource.all = [];
  vi.stubGlobal("EventSource", FakeEventSource);
});
afterEach(() => vi.unstubAllGlobals());

function open() {
  return renderAt("/", "/", <OperatorConversation id={CONV} />);
}

const composer = () => screen.findByLabelText("Ask the Operator");

test("sending_posts_the_turn_and_shows_the_question_at_once", async () => {
  routeOperator();
  route(SEND, (init) => {
    const pair = sentPair((init.body as { turn: string }).turn, "Why?");
    // The hub now holds the pair, so the refetch after the send agrees with it.
    route("GET /api/v1/conversations/{id}", () =>
      conversation({
        message_count: 2,
        messages: [pair.user, pair.assistant],
        turn_in_progress: { turn: pair.assistant.turn, message: pair.assistant.id },
      }),
    );
    return pair;
  });
  open();

  await userEvent.type(await composer(), "Why?{Enter}");

  expect(await screen.findByText("Why?", { selector: "span" })).toBeVisible();
  const body = made(SEND)[0]?.init.body as Record<string, unknown>;
  expect(Object.keys(body).sort()).toEqual(["text", "turn"]);
  expect(body.turn).toMatch(/^[0-9a-f-]{36}$/);
  expect(body.text).toBe("Why?");
});

test("stop_calls_cancel_with_the_turn_id", async () => {
  routeOperator(pendingConversation());
  route(CANCEL, () => ({ status: "cancelled" }));
  open();

  await userEvent.click(await screen.findByRole("button", { name: "Stop" }));

  await waitFor(() => expect(made(CANCEL)).toHaveLength(1));
  expect(made(CANCEL)[0]?.init.params?.path).toMatchObject({ id: CONV, turn: TURN });
});

test("failed_turns_show_words_and_retry", async () => {
  routeOperator(failedConversation());
  route(RETRY, () => sentPair(OTHER_TURN, "Why is the NAS slow?"));
  open();

  expect(await screen.findByText(/budget/i)).toBeVisible();
  expect(screen.queryByText("budget_spent")).toBeNull();
  await userEvent.click(screen.getByRole("button", { name: "Retry" }));

  await waitFor(() => expect(made(RETRY)).toHaveLength(1));
  expect(made(RETRY)[0]?.init.params?.path).toMatchObject({ id: CONV, message: ANSWER });
});

test("retry_is_offered_on_the_last_answer_only", async () => {
  const failed = (id: string, ordinal: number, turn: string, code: string) =>
    message({
      id,
      ordinal,
      role: "assistant",
      turn,
      status: "failed",
      failure: code,
      text: "",
    });
  routeOperator(
    conversation({
      messages: [
        message({ id: "q0", ordinal: 1, turn: "t0" }),
        failed("a0", 2, "t0", "busy"),
        message({ id: "q1", ordinal: 3, turn: "t1" }),
        failed("a1", 4, "t1", "model_unavailable"),
      ],
    }),
  );
  open();

  await screen.findAllByRole("button", { name: "Retry" });
  expect(screen.getAllByRole("button", { name: "Retry" })).toHaveLength(1);
});

test("draft_is_kept_when_the_send_fails_and_cleared_once_it_commits", async () => {
  routeOperator();
  route(SEND, () => failure(500, "internal", "boom"));
  open();

  const box = await composer();
  await userEvent.type(box, "Please check the disks{Enter}");

  await waitFor(() => expect(box).toHaveValue("Please check the disks"));
  expect(localStorage.getItem(`operator:draft:${CONV}`)).toBe("Please check the disks");

  route(SEND, (init) => sentPair((init.body as { turn: string }).turn, "Please check the disks"));
  await userEvent.click(screen.getByRole("button", { name: "Send" }));
  await waitFor(() => expect(localStorage.getItem(`operator:draft:${CONV}`)).toBeNull());
  expect(made(SEND)).toHaveLength(2);
});

test("a_saved_draft_comes_back_when_the_conversation_is_opened_again", async () => {
  routeOperator();
  localStorage.setItem(`operator:draft:${CONV}`, "half a thought");
  open();

  expect(await composer()).toHaveValue("half a thought");
});

test("the_conversation_scope_does_not_follow_the_host_selector", async () => {
  routeOperator(globalConversation());
  localStorage.setItem("selected-host", "nas");
  open();

  expect(await screen.findByText("About all hosts")).toBeVisible();
  expect(screen.getByText(/answers from a summary of every host/)).toBeVisible();
});

test("an_offline_host_conversation_says_so", async () => {
  routeOperator();
  route("GET /api/v1/hosts", () => [
    { id: "host-nas", name: "nas", connected: false, kind: "remote" },
  ]);
  open();

  expect(await screen.findByText("nas is offline")).toBeVisible();
});

test("no_model_replaces_the_composer_with_the_setup_link", async () => {
  routeOperator();
  route("GET /api/v1/settings", () => ({ triage: { enabled: false } }));
  open();

  expect(await screen.findByRole("link", { name: "Set up a model" })).toBeVisible();
  expect(screen.queryByLabelText("Ask the Operator")).toBeNull();
});

test("a_stale_turn_never_appears_and_the_current_turn_streams", async () => {
  routeOperator(pendingConversation());
  open();
  await screen.findByRole("button", { name: "Stop" });
  const source = FakeEventSource.all.at(-1);
  expect(source?.url).toContain(`topics=conversations:${CONV}`);

  act(() =>
    source?.emit("live.text", {
      conversation: CONV,
      turn: OTHER_TURN,
      attempt: 1,
      text: "from a stale turn ",
    }),
  );
  act(() =>
    source?.emit("live.text", {
      conversation: CONV,
      turn: TURN,
      attempt: 1,
      text: "Two restarts ",
    }),
  );

  expect(await screen.findByText("Two restarts")).toBeVisible();
  expect(screen.queryByText("from a stale turn")).toBeNull();
});

test("the_saved_answer_replaces_the_live_text", async () => {
  let current = pendingConversation();
  routeOperator(current);
  route("GET /api/v1/conversations/{id}", () => current);
  open();
  await screen.findByRole("button", { name: "Stop" });
  const source = FakeEventSource.all.at(-1);
  act(() =>
    source?.emit("live.text", {
      conversation: CONV,
      turn: TURN,
      attempt: 1,
      text: "Draft words ",
    }),
  );
  expect(await screen.findByText("Draft words")).toBeVisible();

  current = conversation({
    title: "Why is the NAS slow?",
    message_count: 2,
    messages: [
      message(),
      message({ id: ANSWER, ordinal: 2, role: "assistant", text: "Final answer.", turn: TURN }),
    ],
  });
  act(() => source?.emit("live.end", { conversation: CONV, turn: TURN, status: "complete" }));

  expect(await screen.findByText("Final answer.")).toBeVisible();
  expect(screen.queryByText("Draft words")).toBeNull();
});
