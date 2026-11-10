import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { calls, resetApi, route } from "../../test/apiMock";
import { nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
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

test("chat_keeps_a_separate_conversation_for_each_host", async () => {
  const user = userEvent.setup();
  renderAt("/chat", "/chat", <ChatPage />);

  const textarea = screen.getByLabelText("Ask about your servers");
  await user.type(textarea, "About everything?");
  await user.click(screen.getByRole("button", { name: "Send" }));
  expect(await screen.findByText("Answer to: About everything?")).toBeVisible();

  const select = screen.getByLabelText("Host");
  await screen.findByRole("option", { name: "pi (offline: no tools)" });
  await user.selectOptions(select, "pi");
  expect(
    screen.queryByText("About everything?"),
    "another host starts a new conversation",
  ).not.toBeInTheDocument();

  await user.type(textarea, "About pi?");
  await user.click(screen.getByRole("button", { name: "Send" }));
  expect(await screen.findByText("Answer to: About pi?")).toBeVisible();

  await user.selectOptions(select, "");
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
