import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, test, vi } from "vitest";
import type { ExperienceSearch } from "../../api/types";
import { HISTORICAL_LABEL } from "../../design/knowledgeText";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import searchGlobal from "../../test/fixtures/experience/search-global.json";
import searchHost from "../../test/fixtures/experience/search-host.json";
import { nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { ExperiencePage } from "./ExperiencePage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const SEARCH = "GET /api/v1/experience/search";
const HOST = searchHost as unknown as ExperienceSearch;
const GLOBAL = searchGlobal as unknown as ExperienceSearch;
const searches = () => calls.filter((call) => call.route === SEARCH);

beforeEach(() => {
  resetApi();
  route("GET /api/v1/hosts", () => [nas]);
});

function show(url: string, answer: () => unknown = () => HOST) {
  route(SEARCH, answer);
  return renderAt(url, "/knowledge/experience", <ExperiencePage />);
}

test("every_past_operation_view_carries_the_historical_label", async () => {
  show("/knowledge/experience?host=nas");
  expect(
    await screen.findByText("Historical evidence, not authorization", { selector: "p, div, h2" }),
  ).toBeVisible();
  expect(screen.getByText(/never lets anything run now/)).toBeVisible();
  expect(HOST.label).toBe(HISTORICAL_LABEL);
  expect(GLOBAL.label).toBe(HISTORICAL_LABEL);
});

test("search_is_host_scoped_by_default_and_asks_to_choose_a_host", async () => {
  const view = show("/knowledge/experience");
  expect(await screen.findByText("Choose a host first")).toBeVisible();
  expect(searches()).toHaveLength(0);
  view.unmount();

  show("/knowledge/experience?host=nas");
  await screen.findByRole("link", { name: HOST.items[0]?.operation.goal ?? "" });
  expect(searches()[0]?.init.params?.query).toEqual({ scope: "host", host: "nas" });
});

test("global_search_is_a_deliberate_click_and_sends_no_host", async () => {
  const user = userEvent.setup();
  show("/knowledge/experience", () => GLOBAL);
  await user.click(await screen.findByRole("button", { name: "Search all hosts" }));
  expect(await screen.findByText("Searching every host")).toBeVisible();
  await waitFor(() => expect(screen.getByTestId("location")).toHaveTextContent("scope=global"));
  const query = searches()[0]?.init.params?.query;
  expect(query).toEqual({ scope: "global" });
  expect(query).not.toHaveProperty("host");
});

test("choosing_all_hosts_in_the_scope_control_keeps_the_host_out_of_the_request", async () => {
  const user = userEvent.setup();
  show("/knowledge/experience?host=nas", () => GLOBAL);
  await screen.findByRole("link", { name: GLOBAL.items[0]?.operation.goal ?? "" });
  await user.click(screen.getByRole("radio", { name: "All hosts" }));
  await waitFor(() => expect(searches().at(-1)?.init.params?.query).toEqual({ scope: "global" }));
});

test("a_blank_search_lists_the_newest_operations_and_typing_searches_after_a_pause", async () => {
  const user = userEvent.setup();
  show("/knowledge/experience?host=nas");
  await screen.findByRole("link", { name: HOST.items[0]?.operation.goal ?? "" });
  expect(searches()[0]?.init.params?.query).not.toHaveProperty("q");
  await user.type(screen.getByRole("searchbox", { name: "Search" }), "plex");
  await waitFor(() =>
    expect(searches().at(-1)?.init.params?.query).toEqual({
      scope: "host",
      host: "nas",
      q: "plex",
    }),
  );
  await waitFor(() => expect(screen.getByTestId("location")).toHaveTextContent("q=plex"));
});

test("results_show_outcome_verified_hosts_snippet_contracts_and_the_source_of_every_narrative_field", async () => {
  const hostile = {
    ...HOST,
    items: [
      {
        ...HOST.items[0],
        operation: {
          ...HOST.items[0]?.operation,
          narrative: [
            {
              ...HOST.items[0]?.operation.narrative[0],
              value: "<b>bold</b> <img src=x onerror=alert(1)>",
            },
            ...(HOST.items[0]?.operation.narrative.slice(1) ?? []),
          ],
        },
        match_pass: "any_word",
      },
    ],
  };
  show("/knowledge/experience?host=nas", () => hostile);
  const card = within(
    await screen.findByRole("group", { name: "restart plex after the disk filled" }),
  );
  expect(card.getByRole("link")).toHaveAttribute(
    "href",
    `/operations/${HOST.items[0]?.operation.operation_id}`,
  );
  expect(card.getByText("Succeeded")).toBeVisible();
  expect(card.getByText("Fix verified")).toBeVisible();
  expect(card.getByText("nas")).toBeVisible();
  expect(card.getByText("Matches some of your words")).toBeVisible();
  expect(card.getByText("Ran skill.restart_container on plex: succeeded")).toBeVisible();
  expect(card.getAllByText("restart plex").length).toBeGreaterThan(0);
  expect(card.getAllByText("Drafted by the model, not checked")).toHaveLength(2);
  expect(card.getByText("Written or confirmed by you")).toBeVisible();
  expect(card.getByText("<b>bold</b> <img src=x onerror=alert(1)>")).toBeVisible();
  expect(document.querySelector("img[src='x']")).toBeNull();
});

test("a_failed_search_is_a_load_error_and_empty_results_say_so", async () => {
  const view = show("/knowledge/experience?host=nas", () =>
    failure(503, "unavailable", "orchestration is not enabled"),
  );
  expect(await screen.findByRole("button", { name: "Retry" })).toBeVisible();
  expect(screen.queryByText("No past operations match")).toBeNull();
  view.unmount();

  const empty = show("/knowledge/experience?host=nas&q=zzz", () => ({ ...HOST, items: [] }));
  expect(await screen.findByText("No past operations match")).toBeVisible();
  empty.unmount();

  show("/knowledge/experience?host=nas", () => ({ ...HOST, items: [] }));
  expect(await screen.findByText("No finished operations yet")).toBeVisible();
});

test("ten_results_say_that_only_the_best_ten_are_shown", async () => {
  const item = HOST.items[0];
  const many = Array.from({ length: 10 }, (_, index) => ({
    ...item,
    operation: { ...item?.operation, operation_id: `op-${index}`, goal: `goal ${index}` },
  }));
  show("/knowledge/experience?host=nas", () => ({ ...HOST, items: many }));
  expect(
    await screen.findByText("Showing the 10 best matches. Narrow the search to see others."),
  ).toBeVisible();
});
