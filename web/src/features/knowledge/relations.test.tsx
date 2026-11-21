import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { failure, resetApi, route } from "../../test/apiMock";
import { knowledgeRelation, RELATIONS_PAGE } from "../../test/knowledgeFixtures";
import { callsTo, lastBody, mockKnowledgeApi } from "../../test/knowledgeRoutes";
import { renderAt } from "../../test/render";
import { KnowledgePage } from "./KnowledgePage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const RELATIONS = "GET /api/v1/knowledge/relations";
const RETRACT = "POST /api/v1/knowledge/relations/{id}/retract";
const ASSERT = "POST /api/v1/knowledge/relations";

beforeEach(() => {
  resetApi();
  mockKnowledgeApi();
  route(RETRACT, () => knowledgeRelation({ status: "retracted" }));
  route(ASSERT, () => knowledgeRelation({ id: "new" }));
});

function show() {
  return renderAt("/knowledge?tab=relations", "/knowledge", <KnowledgePage />);
}

test("relations_tab_lists_retracts_and_opens_the_detail_dialog", async () => {
  const user = userEvent.setup();
  show();

  const table = await screen.findByRole("table", { name: "Relations" });
  expect(
    await within(table).findByRole("button", { name: "plex on nas runs on nas" }),
  ).toBeVisible();
  expect(within(table).getAllByRole("row")).toHaveLength(RELATIONS_PAGE.items.length + 1);

  await user.click(within(table).getByRole("button", { name: "plex on nas runs on nas" }));
  const details = await screen.findByRole("dialog", { name: "Relation" });
  for (const label of ["From", "Relation", "To", "Source", "Status", "Observed", "Recorded by"]) {
    expect(within(details).getByText(label, { selector: "dt" })).toBeVisible();
  }
  const recordedBy = within(details).getByText("Recorded by", { selector: "dt" }).parentElement;
  expect(await within(recordedBy as HTMLElement).findByText("You")).toBeVisible();
  await user.click(within(details).getByRole("button", { name: "Close" }));

  await user.click(within(table).getAllByRole("button", { name: "Retract" })[0] as HTMLElement);
  const confirm = await screen.findByRole("dialog", { name: /^Retract/ });
  await user.click(within(confirm).getByRole("button", { name: "Retract" }));

  await waitFor(() => expect(callsTo(RETRACT)).toHaveLength(1));
  expect(lastBody(RETRACT)).toEqual({ expected_version: RELATIONS_PAGE.items[0]?.version });
});

test("retracted_relations_are_requested_only_when_asked_and_offer_no_retract", async () => {
  const user = userEvent.setup();
  route(RELATIONS, ({ params }) => ({
    items: params?.query?.include_retracted
      ? [
          knowledgeRelation({
            id: "gone",
            status: "retracted",
            retracted_at: "2026-11-02T08:00:00Z",
          }),
        ]
      : RELATIONS_PAGE.items,
    next_cursor: null,
  }));
  show();
  await screen.findByRole("table", { name: "Relations" });
  expect(callsTo(RELATIONS)[0]?.init.params?.query).not.toHaveProperty("include_retracted");

  await user.click(screen.getByRole("checkbox", { name: "Show retracted" }));

  await waitFor(() =>
    expect(callsTo(RELATIONS).at(-1)?.init.params?.query).toMatchObject({
      include_retracted: true,
    }),
  );
  const table = await screen.findByRole("table", { name: "Relations" });
  expect(await within(table).findByText("Retracted")).toBeVisible();
  expect(within(table).queryByRole("button", { name: "Retract" })).toBeNull();
});

test("a_purged_relation_says_it_was_removed", async () => {
  route(RELATIONS, () => ({
    items: [knowledgeRelation({ from: null, to: null, content_purged: true, status: "retracted" })],
    next_cursor: null,
  }));
  show();

  expect(
    await screen.findByRole("button", { name: "Removed after the retention period." }),
  ).toBeVisible();
});

test("relations_empty_and_failed_loads_are_different_things", async () => {
  route(RELATIONS, () => ({ items: [], next_cursor: null }));
  const first = show();
  expect(await screen.findByText("No relations yet")).toBeVisible();
  first.unmount();

  route(RELATIONS, () => failure(500, "internal", "boom"));
  show();
  expect(await screen.findByRole("button", { name: "Retry" })).toBeVisible();
});

test("adding_a_relation_sends_from_kind_to_and_shows_the_hubs_400_inside_the_dialog", async () => {
  const user = userEvent.setup();
  route(ASSERT, () => failure(400, "bad_request", "a host cannot run on a container"));
  show();
  await screen.findByRole("table", { name: "Relations" });

  await user.click(screen.getByRole("button", { name: "Add a relation" }));
  const dialog = await screen.findByRole("dialog", { name: "Add a relation" });
  const from = within(within(dialog).getByRole("group", { name: "From" }));
  await user.selectOptions(from.getByRole("combobox", { name: "From is" }), "container");
  await user.selectOptions(await from.findByRole("combobox", { name: "Host" }), "host-nas");
  await user.type(from.getByRole("textbox", { name: "Container name" }), "plex");
  await user.selectOptions(
    within(dialog).getByRole("combobox", { name: "Relation" }),
    "backs_up_to",
  );
  const to = within(within(dialog).getByRole("group", { name: "To" }));
  await user.selectOptions(await to.findByRole("combobox", { name: "Host" }), "host-nas");
  await user.click(within(dialog).getByRole("button", { name: "Save relation" }));

  expect(await within(dialog).findByText(/a host cannot run on a container/i)).toBeVisible();
  expect(lastBody(ASSERT)).toEqual({
    from: "container:host-nas/plex",
    kind: "backs_up_to",
    to: "host:host-nas",
  });
});
