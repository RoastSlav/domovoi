import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { MAX_HISTORY_TURNS } from "../../components/ChatThread";
import { SELECTED_HOST_KEY } from "../../app/HostContext";
import { ChatPage } from "./ChatPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

beforeEach(() => {
  resetApi();
  sessionStorage.clear();
  route("GET /api/v1/hosts", () => [nas, { ...nas, id: "host-pi", name: "pi", connected: false }]);
  route("POST /api/v1/chat", (init) => {
    const { question } = init.body as { question: string };
    return { answer: `Answer to: ${question}` };
  });
});

test("chat_sends_the_earlier_turns_and_clear_empties_them", async () => {
  const user = userEvent.setup();
  renderAt("/chat", "/chat", <ChatPage />);

  expect(
    screen.getByText(
      "Read-only. With a host chosen, it can look at that host's containers, logs, journal and read skills.",
    ),
    "the hint says what the chat may do",
  ).toBeVisible();

  const textarea = screen.getByLabelText("Ask about your servers");
  await user.type(textarea, "Why is the NAS slow?");
  await user.click(screen.getByRole("button", { name: "Send" }));
  expect(await screen.findByText("Answer to: Why is the NAS slow?")).toBeVisible();

  await user.type(textarea, "And now?");
  await user.click(screen.getByRole("button", { name: "Send" }));
  expect(await screen.findByText("Answer to: And now?")).toBeVisible();

  const posts = calls.filter((call) => call.route === "POST /api/v1/chat");
  expect(posts, "the question reached the hub once").toHaveLength(2);
  expect(posts[1]?.init.body, "the earlier turns are sent back").toEqual({
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
  renderAt("/chat", "/chat", <ChatPage />);

  const textarea = screen.getByLabelText("Ask about your servers");
  for (let n = 0; n < 12; n += 1) {
    await user.type(textarea, `Question ${n}`);
    await user.click(screen.getByRole("button", { name: "Send" }));
    expect(await screen.findByText(`Answer to: Question ${n}`)).toBeVisible();
  }

  const posts = calls.filter((call) => call.route === "POST /api/v1/chat");
  expect(posts, "every question still reaches the hub").toHaveLength(12);
  const last = posts[11]?.init.body as {
    history: { role: string; text: string }[];
  };
  expect(last.history, "the hub's limit is never exceeded").toHaveLength(MAX_HISTORY_TURNS);
  expect(last.history[0], "the oldest pair is dropped together").toEqual({
    role: "user",
    text: "Question 1",
  });
  expect(last.history.at(-1)).toEqual({
    role: "assistant",
    text: "Answer to: Question 10",
  });
  expect(
    posts.every(
      (post) => (post.init.body as { history: unknown[] }).history.length <= MAX_HISTORY_TURNS,
    ),
    "no request ever sends more than the limit",
  ).toBe(true);
});

test("chat_says_when_the_monthly_model_budget_is_spent", async () => {
  const user = userEvent.setup();
  route("POST /api/v1/chat", () =>
    failure(503, "budget_spent", "the monthly model budget is spent"),
  );
  renderAt("/chat", "/chat", <ChatPage />);

  await user.type(screen.getByLabelText("Ask about your servers"), "Why?");
  await user.click(screen.getByRole("button", { name: "Send" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("The monthly model budget is spent");
});

test("chat_says_when_the_model_is_not_available", async () => {
  const user = userEvent.setup();
  route("POST /api/v1/chat", () =>
    failure(503, "unavailable", "the model is not available: connection refused"),
  );
  renderAt("/chat", "/chat", <ChatPage />);

  await user.type(screen.getByLabelText("Ask about your servers"), "Why?");
  await user.click(screen.getByRole("button", { name: "Send" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "The model is not available right now",
  );
});

test("chat_keeps_a_separate_conversation_for_each_host", async () => {
  const user = userEvent.setup();
  renderAt("/chat", "/chat", <ChatPage />);

  // Each host has its own conversation, and with it its own composer.
  const textarea = () => screen.getByLabelText("Ask about your servers");
  await user.type(textarea(), "About everything?");
  await user.click(screen.getByRole("button", { name: "Send" }));
  expect(await screen.findByText("Answer to: About everything?")).toBeVisible();

  const select = () => screen.getByLabelText("Host");
  await screen.findByRole("option", { name: "pi (offline: no tools)" });
  await user.selectOptions(select(), "pi");
  expect(
    screen.queryByText("About everything?"),
    "another host starts a new conversation",
  ).not.toBeInTheDocument();

  await user.type(textarea(), "About pi?");
  await user.click(screen.getByRole("button", { name: "Send" }));
  expect(await screen.findByText("Answer to: About pi?")).toBeVisible();

  await user.selectOptions(select(), "");
  expect(
    await screen.findByText("Answer to: About everything?"),
    "the first host's conversation is still there",
  ).toBeVisible();
  expect(screen.queryByText("Answer to: About pi?")).not.toBeInTheDocument();

  const posts = calls.filter((call) => call.route === "POST /api/v1/chat");
  expect(posts[1]?.init.body, "a chosen host is sent with the question").toEqual({
    host: "pi",
    history: [],
    question: "About pi?",
  });
});

test("chat_starts_on_the_selected_host_and_changing_it_here_changes_the_selection", async () => {
  const user = userEvent.setup();
  renderAt("/chat?host=pi", "/chat", <ChatPage />);
  await screen.findByRole("option", { name: "pi (offline: no tools)" });
  expect(screen.getByLabelText("Host")).toHaveValue("pi");

  await user.type(screen.getByLabelText("Ask about your servers"), "Is it up?");
  await user.click(screen.getByRole("button", { name: "Send" }));
  await screen.findByText("Answer to: Is it up?");
  const posts = calls.filter((call) => call.route === "POST /api/v1/chat");
  expect(posts[0]?.init.body).toEqual({ host: "pi", history: [], question: "Is it up?" });

  await user.selectOptions(screen.getByLabelText("Host"), "nas");
  expect(screen.getByTestId("location")).toHaveTextContent("/chat?host=nas");
  expect(localStorage.getItem(SELECTED_HOST_KEY)).toBe("nas");
  expect(screen.getByLabelText("Host")).toHaveValue("nas");
});
