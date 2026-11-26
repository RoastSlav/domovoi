import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { resetApi, route } from "../../test/apiMock";
import { CONV, conversation, message, pendingConversation } from "../../test/conversationFixtures";
import { FakeEventSource } from "../../test/fakeEventSource";
import { nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { stubSlotEnvironment } from "../../test/rightSlot";
import { Layout } from "../../app/Layout";
import { made, routeOperator } from "./operatorTestKit";

vi.mock("../../api/client", () => import("../../test/apiMock"));
vi.mock("../../api/events", async (original) => ({
  ...(await original<typeof import("../../api/events")>()),
  useLiveEvents: () => ({ status: "live", paused: false }),
}));

const QUIET = {
  incidents_action_needed: 0,
  incidents_open: 0,
  hosts_offline: 0,
  hosts_total: 1,
};

beforeEach(() => {
  resetApi();
  FakeEventSource.all = [];
  vi.stubGlobal("EventSource", FakeEventSource);
  stubSlotEnvironment({ width: 1300 });
  route("GET /api/v1/session", () => ({ user: { username: "admin" }, csrf_token: "t" }));
  route("GET /api/v1/overview", () => QUIET);
  route("GET /api/v1/system/update", () => ({ current: "0.1.0", latest: null, newer: false }));
  route("GET /api/v1/incidents", () => ({ items: [], next_cursor: null }));
  route("GET /api/v1/hosts", () => [nas]);
});
afterEach(() => vi.unstubAllGlobals());

const toggle = () => screen.findByRole("button", { name: /the Operator$/ });

function open(url = "/hosts") {
  return renderAt(url, "/*", <Layout />);
}

test("panel_opens_focuses_the_heading_and_escape_returns_focus_to_the_button", async () => {
  routeOperator();
  localStorage.setItem("operator:active", CONV);
  open();
  const button = await toggle();

  await userEvent.click(button);

  const panel = await screen.findByRole("complementary", { name: "Operator" });
  expect(panel).toBeVisible();
  expect(await screen.findByLabelText("Ask the Operator")).toBeVisible();
  await userEvent.keyboard("{Escape}");
  await waitFor(() => expect(screen.queryByRole("complementary", { name: "Operator" })).toBeNull());
  expect(button).toHaveFocus();
});

test("panel_button_is_disabled_on_the_workspace_with_a_reason", async () => {
  routeOperator();
  open("/operator");

  const button = await toggle();
  expect(button).toHaveAttribute("aria-disabled", "true");
  expect(button).toHaveAccessibleDescription("The Operator is open on this page.");
});

test("closing_panel_does_not_stop_operation", async () => {
  routeOperator(pendingConversation());
  localStorage.setItem("operator:active", CONV);
  open();

  await userEvent.click(await toggle());
  await screen.findByRole("button", { name: "Stop" });
  await userEvent.click(screen.getByRole("button", { name: "Close Operator" }));
  await waitFor(() => expect(screen.queryByRole("complementary", { name: "Operator" })).toBeNull());

  expect(made("POST /api/v1/conversations/{id}/turns/{turn}/cancel")).toHaveLength(0);
  expect(made("DELETE /api/v1/conversations/{id}")).toHaveLength(0);
  await userEvent.click(await toggle());
  expect(await screen.findByRole("button", { name: "Stop" })).toBeVisible();
});

test("draft_survives_closing_the_panel", async () => {
  routeOperator();
  localStorage.setItem("operator:active", CONV);
  open();

  await userEvent.click(await toggle());
  await userEvent.type(await screen.findByLabelText("Ask the Operator"), "unfinished thought");
  await userEvent.click(screen.getByRole("button", { name: "Close Operator" }));
  await userEvent.click(await toggle());

  expect(await screen.findByLabelText("Ask the Operator")).toHaveValue("unfinished thought");
});

test("the_panel_starts_a_new_conversation_when_none_is_remembered", async () => {
  routeOperator();
  open();

  await userEvent.click(await toggle());

  expect(await screen.findByRole("button", { name: "Start conversation" })).toBeVisible();
});

test("the_palette_offers_ask_the_operator", async () => {
  routeOperator(conversation({ messages: [message()], message_count: 1 }));
  localStorage.setItem("operator:active", CONV);
  open();
  await toggle();

  await userEvent.keyboard("{Control>}k{/Control}");
  await userEvent.click(await screen.findByRole("option", { name: /Ask the Operator/ }));

  expect(await screen.findByRole("complementary", { name: "Operator" })).toBeVisible();
});
