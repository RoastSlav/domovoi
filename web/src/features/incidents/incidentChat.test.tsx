import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Turn } from "../../api/types";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { detail, nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { MAX_QUESTION_CHARS } from "./IncidentChat";
import { IncidentDetailPage } from "./IncidentDetailPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const ASK = "POST /api/v1/incidents/{id}/ask";

beforeEach(() => {
  resetApi();
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/incidents/{id}", () => detail());
  route("GET /api/v1/incidents/{id}/followups", () => ({ turns: [] }));
  route("GET /api/v1/settings", () => ({ triage: { enabled: true } }));
});

function open() {
  return renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);
}

function asked() {
  return calls.filter((call) => call.route === ASK);
}

const composer = () => screen.findByLabelText("Ask about this incident");

test("chat_hint_says_what_the_model_can_do_and_how_to_send", async () => {
  open();

  const box = await composer();
  const hint = document.getElementById(box.getAttribute("aria-describedby") ?? "");
  expect(hint, "the hint describes the composer").toHaveTextContent(
    "Uses read-only tools on nas. It cannot change anything. Enter sends, Shift+Enter starts a new line.",
  );
  expect(hint).toBeVisible();
});

test("chat_shows_the_saved_turns", async () => {
  const turns: Turn[] = [
    {
      at: "2026-10-31T12:02:00Z",
      asked_by: "user:1",
      question: "Was it the backup job?",
      answer: "Yes, the **backup job** grew it.",
    },
    {
      at: "2026-10-31T12:05:00Z",
      asked_by: "user:1",
      question: "Should I prune them?",
      answer: "Wait for the **last sync** to finish.",
    },
  ];
  route("GET /api/v1/incidents/{id}/followups", () => ({ turns: [...turns] }));
  open();

  const chat = within(await screen.findByRole("region", { name: "Chat" }));
  expect(await chat.findByText("Was it the backup job?")).toBeVisible();
  expect(chat.getByText("Should I prune them?")).toBeVisible();
  expect((await chat.findByText("backup job")).tagName).toBe("STRONG");
  expect(chat.getByText("last sync").tagName).toBe("STRONG");
});

test("chat_shows_the_question_then_its_answer", async () => {
  const user = userEvent.setup();
  const turns: Turn[] = [];
  const answer = "Because the nightly backup kept every snapshot.";
  route("GET /api/v1/incidents/{id}/followups", () => ({ turns: [...turns] }));
  let reply: (value: unknown) => void = () => undefined;
  route(ASK, (init) => {
    turns.push({
      at: "2026-10-31T12:07:00Z",
      asked_by: "user:1",
      question: (init.body as { question: string }).question,
      answer,
    });
    return new Promise((resolve) => (reply = resolve));
  });
  open();

  await user.type(await composer(), "Why now?");
  await user.click(screen.getByRole("button", { name: "Send" }));

  expect(asked(), "the hub got the question once").toHaveLength(1);
  expect(await screen.findByText("Why now?"), "the question stays visible").toBeVisible();
  expect(screen.getByText("Thinking…")).toBeVisible();
  expect(await composer(), "the composer is emptied at once").toHaveValue("");

  reply({ answer });
  expect(
    await screen.findByText(/^Because the nightly backup kept every snapshot\.$/),
    "the answer lands when the remembered turns refresh",
  ).toBeVisible();
  expect(screen.queryByText("Thinking…")).toBeNull();
});

test("chat_clear_button_forgets_the_conversation_after_confirming", async () => {
  const user = userEvent.setup();
  const turns: Turn[] = [
    {
      at: "2026-10-31T12:02:00Z",
      asked_by: "user:1",
      question: "Was it the backup job?",
      answer: "Yes.",
    },
  ];
  route("GET /api/v1/incidents/{id}/followups", () => ({ turns: [...turns] }));
  route("POST /api/v1/incidents/{id}/followups/clear", () => {
    turns.length = 0;
  });
  open();

  const chat = within(await screen.findByRole("region", { name: "Chat" }));
  expect(await chat.findByText("Was it the backup job?")).toBeVisible();

  await user.click(await screen.findByRole("button", { name: "Clear conversation" }));
  const dialog = screen.getByRole("dialog", { name: "Clear the conversation?" });
  expect(
    within(dialog).getByText(
      "The model forgets this conversation. The questions stay in the timeline.",
    ),
    "the dialog says what clearing does",
  ).toBeVisible();

  await user.click(within(dialog).getByRole("button", { name: "Clear conversation" }));
  expect(
    calls.filter((call) => call.route === "POST /api/v1/incidents/{id}/followups/clear"),
    "the hub cleared the conversation",
  ).toHaveLength(1);
  expect(await screen.findByText("Conversation cleared")).toBeVisible();
  expect(
    screen.queryByRole("dialog", { name: "Clear the conversation?" }),
    "the dialog closes once the hub clears",
  ).toBeNull();
});

test("chat_enter_sends_the_question", async () => {
  const user = userEvent.setup();
  route(ASK, () => ({ answer: "Noted." }));
  open();

  await user.type(await composer(), "Why now?");
  await user.keyboard("{Enter}");
  expect(asked(), "Enter alone sent the question").toHaveLength(1);
  expect(asked()[0]?.init.body, "and left no newline in it").toEqual({ question: "Why now?" });
});

test("chat_shift_enter_stays_in_the_text", async () => {
  const user = userEvent.setup();
  open();

  const box = await composer();
  await user.type(box, "First line");
  await user.keyboard("{Shift>}{Enter}{/Shift}");
  expect(asked(), "Shift+Enter is only a line break").toHaveLength(0);
  expect(box, "Shift+Enter broke the line in the text").toHaveValue("First line\n");
});

test("chat_caps_the_question_at_the_hub_limit", async () => {
  const user = userEvent.setup();
  open();

  const box = await composer();
  expect(box).toHaveAttribute("maxlength", String(MAX_QUESTION_CHARS));
  await user.click(box);
  await user.paste("a".repeat(MAX_QUESTION_CHARS - 50));
  expect(
    screen.getByText(`${MAX_QUESTION_CHARS - 50} / ${MAX_QUESTION_CHARS}`),
    "near the limit it counts",
  ).toBeVisible();
});

test("send_says_why_it_is_disabled_while_there_is_no_question", async () => {
  const user = userEvent.setup();
  route(ASK, () => ({ answer: "Noted." }));
  open();

  const send = await screen.findByRole("button", { name: "Send" });
  expect(send).toHaveAttribute("aria-disabled", "true");
  expect(send).toHaveAccessibleDescription("Write a question first.");

  await user.type(await composer(), "Why now?");
  await user.click(send);
  expect(await screen.findByText(/^Noted\.$/)).toBeVisible();
  expect(await composer(), "the composer is emptied").toHaveValue("");
  expect(send, "Send cannot resubmit").toHaveAttribute("aria-disabled", "true");
});

test("a_failed_answer_keeps_the_question_and_offers_retry", async () => {
  const user = userEvent.setup();
  let sick = true;
  route(ASK, () => (sick ? failure(500, "internal", "the hub is sick") : { answer: "Noted." }));
  open();

  await user.type(await composer(), "Why now?");
  await user.click(screen.getByRole("button", { name: "Send" }));

  const alert = await screen.findByRole("alert");
  expect(alert).toHaveTextContent("Could not get an answer");
  expect(alert).toHaveTextContent("The hub ran into a problem. Its log has the details.");
  expect(screen.getByText("Why now?"), "the question is kept above the error").toBeVisible();
  expect(screen.queryByText("Thinking…")).toBeNull();

  sick = false;
  await user.click(within(alert).getByRole("button", { name: "Retry" }));
  expect(await screen.findByText(/^Noted\.$/)).toBeVisible();
  expect(asked().map((call) => call.init.body)).toEqual([
    { question: "Why now?" },
    { question: "Why now?" },
  ]);
  expect(screen.queryByRole("alert")).toBeNull();
});

test("a_rate_limited_question_counts_down", async () => {
  const user = userEvent.setup();
  route(ASK, () => failure(429, "too_many_requests", "slow down", { "Retry-After": "28" }));
  open();

  await user.type(await composer(), "Why now?");
  await user.click(screen.getByRole("button", { name: "Send" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(/Try again in 2[78] s\./);
});

test("with_triage_off_the_composer_is_disabled_and_says_why", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/settings", () => ({ triage: { enabled: false } }));
  open();

  const reason = "Triage is off, so there is no model to ask. Turn it on under Settings, Triage.";
  expect(
    await screen.findByText(reason),
    "the reason is visible, not only a tooltip",
  ).toBeVisible();
  const box = await composer();
  expect(box).toHaveAttribute("aria-disabled", "true");
  expect(box).toHaveAttribute("readonly");
  expect(screen.getByRole("button", { name: "Send" })).toHaveAccessibleDescription(reason);

  await user.type(box, "Why now?{Enter}");
  expect(asked(), "nothing is sent").toHaveLength(0);
});

test("a_refusal_for_no_model_disables_the_composer_until_retry_works", async () => {
  const user = userEvent.setup();
  route(ASK, () => failure(503, "triage_off", "triage is not configured"));
  open();

  await user.type(await composer(), "Why now?");
  await user.click(screen.getByRole("button", { name: "Send" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Triage is off, so there is no model to ask.",
  );
  expect(await composer()).toHaveAttribute("aria-disabled", "true");
  expect(screen.getByRole("button", { name: "Retry" })).toBeVisible();
  expect(calls.at(-1)?.init.body).toEqual({ question: "Why now?" });
});

test("no_model_is_not_reported_as_a_conversation_that_failed_to_load", async () => {
  route("GET /api/v1/incidents/{id}/followups", () =>
    failure(503, "triage_off", "triage is not configured"),
  );
  open();

  expect(await screen.findByText("Triage is off, so there is no model to ask.")).toBeVisible();
  expect(await composer()).toHaveAttribute("aria-disabled", "true");
  expect(screen.queryByText("Could not load the conversation")).toBeNull();
});

test("chat_shows_the_answer_from_the_response_when_the_hub_never_records_the_turn", async () => {
  const user = userEvent.setup();
  route(ASK, () => ({ answer: "Noted." }));
  // The follow-ups route always answers with no turns: the hub never records the
  // question, exactly the race where storing it failed.
  open();

  const chat = within(await screen.findByRole("region", { name: "Chat" }));
  await user.type(await composer(), "Why now?");
  await user.click(screen.getByRole("button", { name: "Send" }));
  expect(await chat.findByText(/^Noted\.$/), "the answer shows without the history").toBeVisible();
  expect(chat.getByText("Why now?"), "the asked question is shown").toBeVisible();
  expect(screen.queryByText("Thinking…"), "the chat is not stuck on Thinking").toBeNull();
});

test("chat_says_when_the_conversation_could_not_be_loaded", async () => {
  let sick = true;
  route("GET /api/v1/incidents/{id}/followups", () =>
    sick ? failure(500, "internal", "the hub is sick") : { turns: [] },
  );
  const user = userEvent.setup();
  open();

  expect(await screen.findByText("Could not load the conversation")).toBeVisible();
  expect(
    screen.queryByText("Thinking…"),
    "a failed history is not read as an empty conversation with a stuck answer",
  ).toBeNull();

  sick = false;
  const chat = within(screen.getByRole("region", { name: "Chat" }));
  await user.click(chat.getByRole("button", { name: "Retry" }));
  await vi.waitFor(() => expect(screen.queryByText("Could not load the conversation")).toBeNull());
});
