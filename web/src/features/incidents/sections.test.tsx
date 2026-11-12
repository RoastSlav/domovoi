import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Action } from "../../api/types";
import { resetApi, route } from "../../test/apiMock";
import { action, detail, nas } from "../../test/fixtures";
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

/** A time `minutes` from now, as the hub writes it. */
function inMinutes(minutes: number): string {
  return new Date(Date.now() + minutes * 60_000).toISOString();
}

/** The actions of an incident with every kind of status, as a long-running one has. */
function manyActions(): Action[] {
  const finished = ["succeeded", "verified", "failed", "denied", "expired", "rejected"];
  return [
    action({ id: "w2", skill: "later_wait", expires_at: inMinutes(50) }),
    action({ id: "w1", skill: "sooner_wait", expires_at: inMinutes(5) }),
    action({ id: "r1", skill: "running_one", status: "dispatched" }),
    ...finished.map((status, n) =>
      action({
        id: `f${n}`,
        skill: `done_${status}`,
        status,
        updated_at: `2026-10-31T12:0${n}:00Z`,
        ...(status === "failed" ? { reason: "exit code 1: no such container" } : {}),
      }),
    ),
  ];
}

const section = (name: RegExp | string) => within(screen.getByRole("region", { name }));

test("actions_are_grouped_by_what_the_person_can_do_about_them", async () => {
  route("GET /api/v1/incidents/{id}", () => ({ ...detail(), actions: manyActions() }));
  open();

  const waiting = await screen.findByRole("region", { name: "Waiting for you · 2" });
  const cards = within(waiting)
    .getAllByRole("group")
    .map((group) => group.getAttribute("aria-label"));
  expect(cards, "full cards only for what waits, the soonest to expire first").toEqual([
    "Action sooner_wait",
    "Action later_wait",
  ]);
  const [soonest, later] = within(waiting).getAllByText(/^Expires in/);
  expect(soonest, "five minutes left is a warning").toHaveAttribute("data-tone", "warning");
  expect(later).not.toHaveAttribute("data-tone");

  const progress = section("In progress · 1");
  expect(progress.getByText("running_one")).toBeVisible();
  expect(progress.getByText("Running on nas. Waiting for the result…")).toBeVisible();
  expect(progress.queryByRole("button", { name: "Approve" })).toBeNull();

  const earlier = section("Earlier actions · 6");
  expect(earlier.getAllByRole("listitem"), "four, then Show all").toHaveLength(4);
  expect(earlier.getByText("done_succeeded")).toBeVisible();
  expect(earlier.queryByRole("group"), "a finished action is a row, not a card").toBeNull();
});

test("a_failed_earlier_action_says_why_on_a_second_line", async () => {
  route("GET /api/v1/incidents/{id}", () => ({ ...detail(), actions: manyActions() }));
  const user = userEvent.setup();
  open();

  const earlier = await screen.findByRole("region", { name: "Earlier actions · 6" });
  await user.click(within(earlier).getByRole("button", { name: "Show all 6" }));
  expect(within(earlier).getAllByRole("listitem")).toHaveLength(6);
  const failed = within(earlier).getByText("done_failed").closest("li") as HTMLElement;
  expect(failed).toHaveTextContent("exit code 1: no such container");
  const fine = within(earlier).getByText("done_succeeded").closest("li") as HTMLElement;
  expect(fine).not.toHaveTextContent("exit code");
});

test("an_expiry_under_fifteen_minutes_is_a_warning_and_later_is_not", async () => {
  route("GET /api/v1/incidents/{id}", () => ({
    ...detail(),
    actions: [action({ expires_at: inMinutes(14) })],
  }));
  const first = open();
  expect(await screen.findByText(/^Expires in/)).toHaveAttribute("data-tone", "warning");
  first.unmount();

  route("GET /api/v1/incidents/{id}", () => ({
    ...detail(),
    actions: [action({ expires_at: inMinutes(17) })],
  }));
  open();
  expect(await screen.findByText(/^Expires in/)).not.toHaveAttribute("data-tone");
});

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

test("running_and_earlier_rows_show_their_arguments_in_full_on_a_second_line", async () => {
  route("GET /api/v1/incidents/{id}", () => ({
    ...detail(),
    actions: [
      action({
        id: "r",
        skill: "restart_container",
        status: "dispatched",
        args: { name: "web", volume: "old-backups-volume-with-a-long-name" },
      }),
      action({ id: "d", skill: "vacuum_db", status: "succeeded", args: { name: "main" } }),
    ],
  }));
  open();

  const progress = await screen.findByRole("region", { name: "In progress · 1" });
  expect(
    within(progress).getByText("name = web · volume = old-backups-volume-with-a-long-name"),
    "every argument, as key = value joined by dots",
  ).toBeVisible();
  const earlier = screen.getByRole("region", { name: "Earlier actions · 1" });
  expect(within(earlier).getByText("name = main")).toBeVisible();
});

test("the_chat_send_button_is_secondary_beside_acknowledge", async () => {
  route("GET /api/v1/incidents/{id}", () => detail());
  open();

  const send = await screen.findByRole("button", { name: "Send" });
  expect(send.className).not.toMatch(/primary/);
});
