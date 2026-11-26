import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { resetApi, route } from "../../test/apiMock";
import { detail, nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { IncidentDetailPage } from "./IncidentDetailPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const MODEL = { kind: "anthropic", api_key: "[set]", model: "claude" };

beforeEach(() => {
  resetApi();
  sessionStorage.clear();
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/incidents/{id}/followups", () => ({ turns: [] }));
  route("GET /api/v1/settings", () => ({ triage: { enabled: true }, llm_primary: MODEL }));
});

function open() {
  return renderAt("/incidents/inc-1", "/incidents/:id", <IncidentDetailPage />);
}

test("long_details_fold_until_asked_for", async () => {
  const user = userEvent.setup();
  const long = ["# Heading", ...Array.from({ length: 12 }, (_, n) => `Line ${n}`)].join("\n\n");
  const d = detail();
  route("GET /api/v1/incidents/{id}", () => ({ ...d, card: { ...d.card, details: long } }));
  open();

  const details = await screen.findByRole("region", { name: "Details" });
  const show = within(details).getByRole("button", { name: "Show full details" });
  expect(show).toHaveAttribute("aria-expanded", "false");
  await user.click(show);
  expect(within(details).getByRole("button", { name: "Show less" })).toHaveAttribute(
    "aria-expanded",
    "true",
  );
});

test("short_details_have_no_toggle", async () => {
  route("GET /api/v1/incidents/{id}", () => detail());
  open();

  const details = await screen.findByRole("region", { name: "Details" });
  expect(within(details).queryByRole("button")).toBeNull();
});

const TURNS = [1, 2, 3].map((n) => ({
  at: `2026-10-31T12:0${n}:00Z`,
  asked_by: "user:1",
  question: `Question ${n}?`,
  answer: `Answer ${n}.`,
}));

test("chat_shows_the_last_two_questions_and_offers_the_earlier_one", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/incidents/{id}", () => detail());
  route("GET /api/v1/incidents/{id}/followups", () => ({ turns: TURNS }));
  open();

  const chat = await screen.findByRole("region", { name: "Chat" });
  expect(await within(chat).findByText("Question 3?")).toBeVisible();
  expect(within(chat).getByText("Question 2?")).toBeVisible();
  expect(within(chat).queryByText("Question 1?")).toBeNull();

  await user.click(within(chat).getByRole("button", { name: "Show 1 earlier question" }));
  expect(within(chat).getByText("Question 1?")).toBeVisible();
  expect(within(chat).queryByRole("button", { name: /earlier question/ })).toBeNull();
});

test("opening_an_incident_with_chat_history_does_not_scroll_the_page", async () => {
  const scroll = vi.fn();
  Element.prototype.scrollIntoView = scroll;
  route("GET /api/v1/incidents/{id}", () => detail());
  route("GET /api/v1/incidents/{id}/followups", () => ({ turns: TURNS }));
  open();

  await screen.findByText("Question 3?");
  expect(scroll, "the page stays where it is on load").not.toHaveBeenCalled();
});

test("sending_a_question_scrolls_only_as_far_as_needed", async () => {
  const user = userEvent.setup();
  const scroll = vi.fn();
  Element.prototype.scrollIntoView = scroll;
  route("GET /api/v1/incidents/{id}", () => detail());
  route("POST /api/v1/incidents/{id}/ask", () => ({ answer: "Noted." }));
  open();

  await user.type(await screen.findByLabelText("Ask about this incident"), "Why?{Enter}");
  await vi.waitFor(() => expect(scroll).toHaveBeenCalled());
  expect(scroll).toHaveBeenCalledWith({ block: "nearest" });
});

test("the_chat_send_button_is_secondary_beside_acknowledge", async () => {
  route("GET /api/v1/incidents/{id}", () => detail());
  open();

  const send = await screen.findByRole("button", { name: "Send" });
  expect(send.className).not.toMatch(/primary/);
});
