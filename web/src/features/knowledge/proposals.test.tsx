import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { failure, resetApi, route } from "../../test/apiMock";
import { FACTS_PAGE, knowledgeFact } from "../../test/knowledgeFixtures";
import { callsTo, lastBody, mockKnowledgeApi } from "../../test/knowledgeRoutes";
import { renderAt } from "../../test/render";
import { KnowledgePage } from "./KnowledgePage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const FACTS = "GET /api/v1/knowledge/facts";
const inferred = FACTS_PAGE.items.filter((fact) => fact.source === "inferred");
const ordinary = inferred.find((fact) => fact.predicate === "role");
const safety = inferred.find((fact) => fact.predicate === "trusted_cidr");

beforeEach(() => {
  resetApi();
  mockKnowledgeApi();
  route("POST /api/v1/knowledge/facts/{id}/accept", () => knowledgeFact({ id: "mine" }));
  route("POST /api/v1/knowledge/facts/{id}/retract", () => knowledgeFact({ status: "retracted" }));
});

function show() {
  return renderAt("/knowledge?tab=proposals", "/knowledge", <KnowledgePage />);
}

function acceptButton(index: number) {
  const button = screen.getAllByRole("button", { name: "Accept" })[index];
  if (!button) {
    throw new Error(`no Accept button at ${index}`);
  }
  return button;
}

test("proposals_list_inferred_facts_and_the_tab_counts_them_without_a_second_request", async () => {
  show();

  expect(await screen.findAllByRole("button", { name: "Accept" })).toHaveLength(inferred.length);
  expect(
    screen.getByRole("tab", { name: new RegExp(`Proposals ${inferred.length} waiting`) }),
  ).toBeVisible();
  const asked = callsTo(FACTS).filter((call) => call.init.params?.query?.source === "inferred");
  expect(asked).toHaveLength(1);
  expect(screen.getByText("media player")).toBeVisible();
});

test("accepting_a_proposal_sends_the_expected_version_and_no_password_for_an_ordinary_fact", async () => {
  const user = userEvent.setup();
  show();
  await screen.findAllByRole("button", { name: "Accept" });

  await user.click(acceptButton(0));

  expect(
    await screen.findByText("Saved as your own fact. The original stays as history."),
  ).toBeVisible();
  expect(lastBody("POST /api/v1/knowledge/facts/{id}/accept")).toEqual({
    expected_version: ordinary?.version,
  });
  expect(screen.queryByRole("dialog")).toBeNull();
});

test("accepting_a_safety_critical_proposal_asks_for_the_password", async () => {
  const user = userEvent.setup();
  show();
  await screen.findAllByRole("button", { name: "Accept" });

  await user.click(acceptButton(1));

  const prompt = await screen.findByRole("dialog", { name: "Confirm with your password" });
  expect(callsTo("POST /api/v1/knowledge/facts/{id}/accept")).toHaveLength(0);
  await user.type(within(prompt).getByLabelText("Password"), "correct horse");
  await user.click(within(prompt).getByRole("button", { name: "Accept" }));

  await waitFor(() => expect(callsTo("POST /api/v1/knowledge/facts/{id}/accept")).toHaveLength(1));
  expect(lastBody("POST /api/v1/knowledge/facts/{id}/accept")).toEqual({
    expected_version: safety?.version,
    password: "correct horse",
  });
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
});

test("retracting_asks_first_names_the_fact_and_never_asks_for_a_password", async () => {
  const user = userEvent.setup();
  show();
  await screen.findAllByRole("button", { name: "Accept" });

  await user.click(screen.getAllByRole("button", { name: "Retract" })[0] as HTMLElement);

  const dialog = await screen.findByRole("dialog", { name: "Retract Role for plex on nas?" });
  expect(within(dialog).queryByLabelText("Password")).toBeNull();
  expect(callsTo("POST /api/v1/knowledge/facts/{id}/retract")).toHaveLength(0);
  await user.click(within(dialog).getByRole("button", { name: "Retract" }));

  expect(await screen.findByText("Retracted. It is no longer used.")).toBeVisible();
  expect(lastBody("POST /api/v1/knowledge/facts/{id}/retract")).toEqual({
    expected_version: ordinary?.version,
  });
});

test("a_stale_version_is_shown_in_the_hubs_words_and_the_list_refetches", async () => {
  const user = userEvent.setup();
  route("POST /api/v1/knowledge/facts/{id}/accept", () =>
    failure(409, "conflict", "someone changed it meanwhile; reload and try again"),
  );
  show();
  await screen.findAllByRole("button", { name: "Accept" });
  const before = callsTo(FACTS).length;

  await user.click(acceptButton(0));

  expect(await screen.findByText(/someone changed it meanwhile/i)).toBeVisible();
  await waitFor(() => expect(callsTo(FACTS).length).toBeGreaterThan(before));
});

test("proposals_empty_state_says_nothing_waits", async () => {
  route(FACTS, () => ({ items: [], next_cursor: null }));
  show();

  expect(await screen.findByText("Nothing waits for your word")).toBeVisible();
  expect(screen.queryByRole("tab", { name: /waiting/ })).toBeNull();
});
