import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { MAX_HISTORY_TURNS } from "../../components/ChatThread";
import { ChatPage } from "./ChatPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

beforeEach(() => {
  resetApi();
  sessionStorage.clear();
  route("GET /api/v1/hosts", () => [nas, { ...nas, id: "host-pi", name: "pi", connected: false }]);
  route("GET /api/v1/settings", () => ({ triage: { enabled: true } }));
  route("POST /api/v1/chat", (init) => {
    const { question } = init.body as { question: string };
    return { answer: `Answer to: ${question}` };
  });
});

function open(url = "/chat") {
  return renderAt(url, "/chat", <ChatPage />);
}

function posts() {
  return calls.filter((call) => call.route === "POST /api/v1/chat");
}

const composer = () => screen.getByLabelText("Ask about your servers");

test("chat_sends_the_earlier_turns_and_clear_empties_them", async () => {
  const user = userEvent.setup();
  open();

  expect(
    screen.getByText(/^Read-only\. It answers from a summary of every host/),
    "the hint says what the chat may do",
  ).toBeVisible();

  await user.type(composer(), "Why is the NAS slow?");
  await user.click(screen.getByRole("button", { name: "Send" }));
  expect(await screen.findByText("Answer to: Why is the NAS slow?")).toBeVisible();

  await user.type(composer(), "And now?");
  await user.click(screen.getByRole("button", { name: "Send" }));
  expect(await screen.findByText("Answer to: And now?")).toBeVisible();

  expect(posts(), "each question reached the hub once").toHaveLength(2);
  expect(posts()[1]?.init.body, "the earlier turns are sent back").toEqual({
    history: [
      { role: "user", text: "Why is the NAS slow?" },
      { role: "assistant", text: "Answer to: Why is the NAS slow?" },
    ],
    question: "And now?",
  });

  await user.click(screen.getByRole("button", { name: "Clear conversation" }));
  expect(
    screen.queryByText("Answer to: Why is the NAS slow?"),
    "Clear empties the conversation",
  ).not.toBeInTheDocument();
  expect(sessionStorage.getItem("chat:all"), "the saved conversation is gone").toBeNull();
});

test("chat_sends_only_the_newest_twenty_earlier_messages", async () => {
  const user = userEvent.setup();
  open();

  for (let n = 0; n < 12; n += 1) {
    await user.type(composer(), `Question ${n}`);
    await user.click(screen.getByRole("button", { name: "Send" }));
    expect(await screen.findByText(`Answer to: Question ${n}`)).toBeVisible();
  }

  expect(posts(), "every question still reaches the hub").toHaveLength(12);
  const last = posts()[11]?.init.body as { history: { role: string; text: string }[] };
  expect(last.history, "the hub's limit is never exceeded").toHaveLength(MAX_HISTORY_TURNS);
  expect(last.history[0], "the oldest pair is dropped together").toEqual({
    role: "user",
    text: "Question 1",
  });
  expect(last.history.at(-1)).toEqual({ role: "assistant", text: "Answer to: Question 10" });
  expect(
    posts().every(
      (post) => (post.init.body as { history: unknown[] }).history.length <= MAX_HISTORY_TURNS,
    ),
    "no request ever sends more than the limit",
  ).toBe(true);
});

test("enter_sends_and_shift_enter_does_not", async () => {
  const user = userEvent.setup();
  open();

  await user.type(composer(), "First line");
  await user.keyboard("{Shift>}{Enter}{/Shift}");
  expect(posts(), "Shift+Enter is only a line break").toHaveLength(0);
  expect(composer()).toHaveValue("First line\n");

  await user.keyboard("second{Enter}");
  expect(posts()).toHaveLength(1);
  expect((posts()[0]?.init.body as { question: string }).question).toBe("First line\nsecond");
  expect(await screen.findByText(/^Answer to: First line/)).toBeVisible();
  expect(composer(), "the composer empties").toHaveValue("");
});

test("a_failed_answer_shows_retry_with_the_question_kept", async () => {
  const user = userEvent.setup();
  let sick = true;
  route("POST /api/v1/chat", () =>
    sick
      ? failure(503, "unavailable", "the model is not available: connection refused")
      : { answer: "It is fine." },
  );
  open();

  await user.type(composer(), "Is it up?");
  await user.click(screen.getByRole("button", { name: "Send" }));

  const alert = await screen.findByRole("alert");
  expect(alert).toHaveTextContent("Could not get an answer");
  expect(alert).toHaveTextContent("The model is not available: connection refused.");
  expect(screen.getByText("Is it up?"), "the question is kept").toBeVisible();
  expect(composer(), "an ordinary failure leaves the composer usable").not.toHaveAttribute(
    "aria-disabled",
  );

  sick = false;
  await user.click(within(alert).getByRole("button", { name: "Retry" }));
  expect(await screen.findByText("It is fine.")).toBeVisible();
  expect(posts().map((post) => (post.init.body as { question: string }).question)).toEqual([
    "Is it up?",
    "Is it up?",
  ]);
});

test("chat_says_when_the_monthly_model_budget_is_spent", async () => {
  const user = userEvent.setup();
  route("POST /api/v1/chat", () =>
    failure(503, "budget_spent", "the monthly model budget is spent"),
  );
  open();

  await user.type(composer(), "Why?");
  await user.click(screen.getByRole("button", { name: "Send" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "The monthly model budget is spent. It resets next month.",
  );
  expect(composer(), "asking again would not help, so the composer says why").toHaveAttribute(
    "aria-disabled",
    "true",
  );
});

test("a_disabled_composer_shows_its_reason", async () => {
  route("GET /api/v1/settings", () => ({ triage: { enabled: false } }));
  open();

  const reason = "Triage is off, so there is no model to ask. Turn it on under Settings, Triage.";
  expect(await screen.findByText(reason)).toBeVisible();
  expect(composer()).toHaveAttribute("aria-disabled", "true");
  expect(screen.getByRole("button", { name: "Send" })).toHaveAccessibleDescription(reason);
});

test("stop_cancels_the_question_and_hands_it_back", async () => {
  const user = userEvent.setup();
  route("POST /api/v1/chat", () => new Promise(() => undefined));
  open();

  await user.type(composer(), "A long one");
  await user.click(screen.getByRole("button", { name: "Send" }));
  expect(await screen.findByText("Thinking…")).toBeVisible();

  await user.click(screen.getByRole("button", { name: "Stop" }));
  await vi.waitFor(() => expect(screen.queryByText("Thinking…")).toBeNull());
  expect(composer(), "the question is back to be edited or sent again").toHaveValue("A long one");
  expect(screen.queryByRole("alert"), "stopping is not an error").toBeNull();
});

test("an_empty_chat_offers_starter_questions_about_the_selected_host", async () => {
  const user = userEvent.setup();
  open("/chat?host=nas");

  const starters = within(screen.getByRole("group", { name: "Suggested questions" }));
  expect(starters.getAllByRole("button").map((button) => button.textContent)).toEqual([
    "Is anything wrong on nas right now?",
    "What changed on nas in the last day?",
    "Which containers on nas restarted recently?",
    "How full are the disks on nas?",
  ]);

  await user.click(starters.getByRole("button", { name: "How full are the disks on nas?" }));
  expect(await screen.findByText("Answer to: How full are the disks on nas?")).toBeVisible();
  expect(posts()[0]?.init.body).toEqual({
    host: "nas",
    history: [],
    question: "How full are the disks on nas?",
  });
  expect(
    screen.queryByRole("group", { name: "Suggested questions" }),
    "starters leave once the conversation has begun",
  ).toBeNull();
});

test("the_header_says_which_host_the_chat_is_about", async () => {
  const all = open();
  expect(screen.getByText("About all hosts")).toBeVisible();
  expect(screen.queryByLabelText("Host"), "the top bar owns the host choice").toBeNull();
  all.unmount();

  open("/chat?host=pi");
  expect(screen.getByText("About pi")).toBeVisible();
  expect(
    await screen.findByText("pi is offline"),
    "an offline host cannot be looked at",
  ).toBeVisible();
  expect(
    screen.getByText(
      /^Read-only\. It can look at the containers, logs, journal and read skills of pi\./,
    ),
  ).toBeVisible();
});

test("chat_keeps_a_separate_conversation_for_each_host", async () => {
  const user = userEvent.setup();
  const everything = open();
  await user.type(composer(), "About everything?");
  await user.click(screen.getByRole("button", { name: "Send" }));
  expect(await screen.findByText("Answer to: About everything?")).toBeVisible();
  everything.unmount();

  const pi = open("/chat?host=pi");
  expect(
    screen.queryByText("About everything?"),
    "another host starts a new conversation",
  ).not.toBeInTheDocument();
  await user.type(composer(), "About pi?");
  await user.click(screen.getByRole("button", { name: "Send" }));
  expect(await screen.findByText("Answer to: About pi?")).toBeVisible();
  expect(posts()[1]?.init.body, "a chosen host is sent with the question").toEqual({
    host: "pi",
    history: [],
    question: "About pi?",
  });
  pi.unmount();

  open();
  expect(
    await screen.findByText("Answer to: About everything?"),
    "the first conversation is still there",
  ).toBeVisible();
  expect(screen.queryByText("Answer to: About pi?")).not.toBeInTheDocument();
});
