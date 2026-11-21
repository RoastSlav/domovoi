import { Route, Routes } from "react-router";
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { failure, resetApi, route } from "../../test/apiMock";
import {
  CONV,
  conversation,
  globalConversation,
  pendingConversation,
} from "../../test/conversationFixtures";
import { FakeEventSource } from "../../test/fakeEventSource";
import { nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { made, routeOperator } from "./operatorTestKit";
import { OperatorPage } from "./OperatorPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const CREATE = "POST /api/v1/conversations";
const DELETE = "DELETE /api/v1/conversations/{id}";

beforeEach(() => {
  resetApi();
  FakeEventSource.all = [];
  vi.stubGlobal("EventSource", FakeEventSource);
  vi.stubGlobal("matchMedia", () => ({
    matches: true,
    addEventListener() {},
    removeEventListener() {},
  }));
});
afterEach(() => vi.unstubAllGlobals());

function page(url = "/operator") {
  return renderAt(
    url,
    "/operator/*",
    <Routes>
      <Route path="/" element={<OperatorPage />} />
      <Route path=":id" element={<OperatorPage />} />
    </Routes>,
  );
}

test("workspace_lists_conversations_and_the_whole_row_is_the_link", async () => {
  routeOperator(conversation({ title: "Why is the NAS slow?" }));
  page();

  const row = await screen.findByRole("link", { name: /Why is the NAS slow\?/ });
  expect(row).toHaveAttribute("href", `/operator/${CONV}`);
  expect(within(row).getByText("About nas")).toBeVisible();
});

test("workspace_says_when_there_are_no_conversations_and_when_loading_fails", async () => {
  routeOperator();
  route("GET /api/v1/conversations", () => ({ items: [], next_cursor: null, as_of_seq: 1 }));
  const first = page();
  expect(await screen.findByText("No conversations yet")).toBeVisible();
  first.unmount();

  route("GET /api/v1/conversations", () => failure(500, "internal", "boom"));
  page();
  expect(await screen.findByRole("button", { name: "Retry" })).toBeVisible();
});

test("global_scope_is_explicit", async () => {
  routeOperator();
  route(CREATE, () => globalConversation());
  page();

  const start = await screen.findByRole("button", { name: "Start conversation" });
  expect(start).toHaveAttribute("aria-disabled", "true");
  expect(screen.getByRole("radio", { name: "All hosts" })).not.toBeChecked();
  expect(screen.getByRole("radio", { name: "One host" })).not.toBeChecked();

  await userEvent.click(screen.getByRole("radio", { name: "All hosts" }));
  await userEvent.click(screen.getByRole("button", { name: "Start conversation" }));

  await waitFor(() => expect(made(CREATE)).toHaveLength(1));
  expect(made(CREATE)[0]?.init.body).toMatchObject({ scope: { kind: "global" } });
});

test("with_a_host_selected_one_host_is_preselected_and_shown", async () => {
  routeOperator();
  localStorage.setItem("selected-host", "nas");
  route(CREATE, () => conversation());
  page();

  expect(await screen.findByRole("radio", { name: "One host" })).toBeChecked();
  expect(await screen.findByRole("combobox", { name: "Host" })).toHaveValue("nas");
  await userEvent.click(screen.getByRole("button", { name: "Start conversation" }));
  await waitFor(() => expect(made(CREATE)).toHaveLength(1));
  expect(made(CREATE)[0]?.init.body).toMatchObject({ scope: { kind: "host", host: "nas" } });
});

test("workspace_new_conversation_prefills_from_the_query", async () => {
  routeOperator();
  route(CREATE, () => conversation({ operation_id: "op-1" }));
  page("/operator?operation=op-1&host=nas");

  expect(await screen.findByText("Operation attached, read-only")).toBeVisible();
  expect(await screen.findByRole("combobox", { name: "Host" })).toHaveValue("nas");
  await userEvent.click(screen.getByRole("button", { name: "Start conversation" }));
  await waitFor(() => expect(made(CREATE)).toHaveLength(1));
  expect(made(CREATE)[0]?.init.body).toMatchObject({ operation_id: "op-1" });
});

test("workspace_notes_that_it_lists_every_hosts_conversations_while_a_host_is_selected", async () => {
  routeOperator();
  localStorage.setItem("selected-host", "nas");
  page();

  expect(await screen.findByText("Showing all hosts")).toBeVisible();
});

test("deleting_asks_for_confirmation_by_name", async () => {
  routeOperator(conversation({ title: "Why is the NAS slow?" }));
  route(DELETE, () => ({}));
  page(`/operator/${CONV}`);

  await userEvent.click(await screen.findByRole("button", { name: "Delete" }));
  const dialog = await screen.findByRole("dialog");
  expect(within(dialog).getByText(/Why is the NAS slow\?/)).toBeVisible();
  expect(made(DELETE)).toHaveLength(0);
  await userEvent.click(within(dialog).getByRole("button", { name: "Delete conversation" }));

  await waitFor(() => expect(made(DELETE)).toHaveLength(1));
});

test("deleting_is_disabled_with_a_reason_while_an_answer_is_pending", async () => {
  routeOperator(pendingConversation());
  route("GET /api/v1/hosts", () => [nas]);
  page(`/operator/${CONV}`);

  const button = await screen.findByRole("button", { name: "Delete" });
  expect(button).toHaveAttribute("aria-disabled", "true");
  expect(button).toHaveAccessibleDescription("An answer is being written. Stop it first.");
});
