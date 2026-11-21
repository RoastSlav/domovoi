import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { failure, resetApi, route } from "../../test/apiMock";
import { renderAt } from "../../test/render";
import { callsTo, factsAnswer, mockKnowledgeApi } from "../../test/knowledgeRoutes";
import {
  FACT_PURGED,
  FACT_RETRACTED,
  FACTS_PAGE,
  knowledgeFact,
} from "../../test/knowledgeFixtures";
import { knowledgeStatusMeta } from "../../design/tone";
import { FACT_SEARCH_DEBOUNCE_MS } from "./FactFilterBar";
import { KnowledgePage } from "./KnowledgePage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const FACTS = "GET /api/v1/knowledge/facts";

beforeEach(() => {
  resetApi();
  mockKnowledgeApi();
});

afterEach(() => {
  vi.useRealTimers();
});

function show(url = "/knowledge") {
  return renderAt(url, "/knowledge", <KnowledgePage />);
}

const inferred = FACTS_PAGE.items.filter((fact) => fact.source === "inferred");

test("facts_tab_lists_facts_with_source_and_status_and_labels_stale_and_disputed", async () => {
  show();

  const table = await screen.findByRole("table", { name: "Facts" });
  expect(within(table).getAllByRole("row")).toHaveLength(FACTS_PAGE.items.length + 1);
  expect(within(table).getAllByText("Inferred by Domovoi")).toHaveLength(inferred.length);
  const stale = within(table).getByText("Stale");
  expect(stale).toHaveAttribute("aria-description", knowledgeStatusMeta("stale").hint);
  const disputed = within(table).getAllByText("Disputed");
  expect(disputed).toHaveLength(2);
  expect(disputed[0]).toHaveAttribute("aria-description", knowledgeStatusMeta("disputed").hint);
  expect(within(table).getByText("media server")).toBeVisible();
  expect(within(table).getByText("192.168.1.0/24")).toBeVisible();
});

test("retracted_and_replaced_facts_are_hidden_until_asked_for", async () => {
  const user = userEvent.setup();
  route(FACTS, ({ params }) => {
    const status = params?.query?.status;
    if (status === "retracted") {
      return { items: [FACT_RETRACTED, FACT_PURGED], next_cursor: null };
    }
    return factsAnswer(params?.query);
  });
  show();
  await screen.findByRole("table", { name: "Facts" });
  expect(callsTo(FACTS)[0]?.init.params?.query).not.toHaveProperty("status");

  await user.click(screen.getByRole("radio", { name: "Retracted" }));

  const table = await screen.findByRole("table", { name: "Facts" });
  await waitFor(() => expect(within(table).getByText("Old backup note")).toBeVisible());
  expect(within(table).getByText("Removed after the retention period.")).toBeVisible();
  expect(callsTo(FACTS).at(-1)?.init.params?.query).toMatchObject({ status: "retracted" });
  expect(screen.getByTestId("location")).toHaveTextContent("status=retracted");
  expect(within(table).queryByRole("button")).toBeNull();
});

test("facts_follow_the_host_selector_and_the_address", async () => {
  show("/knowledge?host=nas&source=user&predicate=role&status=stale");

  await screen.findByRole("table", { name: "Facts" });
  const query = callsTo(FACTS)[0]?.init.params?.query;
  expect(query).toMatchObject({
    host: "nas",
    source: "user",
    predicate: "role",
    status: "stale",
    limit: 50,
  });
  expect(screen.getByRole("combobox", { name: "Source" })).toHaveValue("user");
  expect(screen.getByRole("combobox", { name: "Kind" })).toHaveValue("role");
  expect(screen.getByRole("radio", { name: "Stale" })).toBeChecked();

  const user = userEvent.setup();
  await user.selectOptions(screen.getByRole("combobox", { name: "Source" }), "discovered");
  expect(screen.getByTestId("location")).toHaveTextContent("source=discovered");
  expect(screen.getByTestId("location")).toHaveTextContent("host=nas");
});

test("search_uses_the_search_route_without_a_cursor_and_says_when_it_is_cut", async () => {
  const many = Array.from({ length: 50 }, (_, index) =>
    knowledgeFact({ id: `hit-${index}`, value: `plex ${index}` }),
  );
  route(FACTS, ({ params }) =>
    params?.query?.q === undefined
      ? factsAnswer(params?.query)
      : { items: many, next_cursor: "never-used" },
  );
  show("/knowledge?q=plex");

  const table = await screen.findByRole("table", { name: "Facts" });
  await within(table).findByText("plex 0");
  const search = callsTo(FACTS).filter((call) => call.init.params?.query?.q !== undefined);
  expect(search).toHaveLength(1);
  expect(search[0]?.init.params?.query).toMatchObject({ q: "plex", limit: 50 });
  expect(search[0]?.init.params?.query).not.toHaveProperty("cursor");
  expect(
    screen.getByText("Showing the best 50 matches. Narrow the search to see others."),
  ).toBeVisible();
  expect(screen.queryByRole("button", { name: "Load more" })).toBeNull();
});

test("typing_a_search_waits_for_a_pause_and_then_writes_it_to_the_address", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  show();
  await screen.findByRole("table", { name: "Facts" });

  fireEvent.change(screen.getByLabelText("Search"), { target: { value: "plex" } });
  await act(() => vi.advanceTimersByTimeAsync(FACT_SEARCH_DEBOUNCE_MS / 2));
  expect(screen.getByTestId("location")).not.toHaveTextContent("q=plex");

  await act(() => vi.advanceTimersByTimeAsync(FACT_SEARCH_DEBOUNCE_MS));
  expect(screen.getByTestId("location")).toHaveTextContent("q=plex");
});

test("a_failed_facts_load_is_a_load_error_not_an_empty_state", async () => {
  const user = userEvent.setup();
  route(FACTS, () => failure(500, "internal", "boom"));
  show();

  expect(await screen.findByRole("button", { name: "Retry" })).toBeVisible();
  expect(screen.queryByText("No facts yet")).toBeNull();

  route(FACTS, ({ params }) => factsAnswer(params?.query));
  await user.click(screen.getByRole("button", { name: "Retry" }));
  expect(await screen.findByRole("table", { name: "Facts" })).toBeVisible();
});

test("facts_empty_states_say_what_to_do", async () => {
  const user = userEvent.setup();
  route(FACTS, () => ({ items: [], next_cursor: null }));
  show();
  expect(await screen.findByText("No facts yet")).toBeVisible();
  expect(screen.getAllByRole("button", { name: "Add a fact" }).length).toBeGreaterThan(0);

  await user.selectOptions(screen.getByRole("combobox", { name: "Source" }), "user");
  expect(await screen.findByText("No facts match")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Clear filters" }));
  expect(await screen.findByText("No facts yet")).toBeVisible();

  await user.click(screen.getByRole("radio", { name: "Retracted" }));
  expect(await screen.findByText("No retracted facts")).toBeVisible();
});

test("load_more_passes_the_cursor", async () => {
  const user = userEvent.setup();
  route(FACTS, ({ params }) =>
    params?.query?.cursor === "next"
      ? { items: [knowledgeFact({ id: "later", value: "second page value" })], next_cursor: null }
      : { items: [knowledgeFact({ id: "first" })], next_cursor: "next" },
  );
  show();

  await user.click(await screen.findByRole("button", { name: "Load more" }));

  expect(await screen.findByText("second page value")).toBeVisible();
  expect(callsTo(FACTS).at(-1)?.init.params?.query).toMatchObject({ cursor: "next" });
  expect(screen.queryByRole("button", { name: "Load more" })).toBeNull();
});

test("waiting_note_appears_only_while_proposals_exist", async () => {
  show();
  const note = await screen.findByText(
    `${inferred.length} facts inferred by Domovoi wait for your word.`,
    { exact: false },
  );
  expect(note).toBeVisible();
  expect(screen.getByRole("link", { name: "Review proposals" })).toHaveAttribute(
    "href",
    "/knowledge?tab=proposals",
  );
});

test("no_waiting_note_without_proposals", async () => {
  route(FACTS, ({ params }) =>
    params?.query?.source === "inferred"
      ? { items: [], next_cursor: null }
      : factsAnswer(params?.query),
  );
  show();

  await screen.findByRole("table", { name: "Facts" });
  expect(screen.queryByText(/wait for your word/)).toBeNull();
});
