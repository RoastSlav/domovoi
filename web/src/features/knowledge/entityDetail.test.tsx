import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { resetApi, route } from "../../test/apiMock";
import { ENTITIES, RELATIONS_PAGE } from "../../test/knowledgeFixtures";
import { callsTo, mockKnowledgeApi } from "../../test/knowledgeRoutes";
import { renderAt } from "../../test/render";
import { EntityDetailPage } from "./EntityDetailPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const FACTS = "GET /api/v1/knowledge/facts";
const RELATIONS = "GET /api/v1/knowledge/relations";
const DELETE = "DELETE /api/v1/knowledge/entities/{id}";
const vpn = ENTITIES[0];

beforeEach(() => {
  resetApi();
  mockKnowledgeApi();
  route(DELETE, () => undefined);
});

function show(id = vpn?.id ?? "") {
  return renderAt(`/knowledge/entities/${id}`, "/knowledge/entities/:id", <EntityDetailPage />);
}

test("entity_page_shows_its_facts_and_relations_by_subject_and_deletes_back_to_the_list", async () => {
  const user = userEvent.setup();
  route(FACTS, () => ({
    items: [
      {
        ...(RELATIONS_PAGE.items[0] ? {} : {}),
        id: "f-1",
        subject: `entity:${vpn?.id}`,
        predicate: "role",
        value: "tunnel to the office",
        source: "user",
        status: "current",
        stored_status: "current",
        version: 1,
        observed_at: "2026-10-31T12:00:00Z",
        valid_until: null,
        last_verified_at: null,
        fresh_until: null,
        supersedes: null,
        evidence: null,
        actor: "user:me",
        created_at: "2026-10-31T12:00:00Z",
        retracted_at: null,
        content_purged: false,
      },
    ],
    next_cursor: null,
  }));
  show();

  expect(await screen.findByRole("heading", { level: 1, name: "Home VPN" })).toBeVisible();
  expect(await screen.findByText("tunnel to the office")).toBeVisible();
  expect(await screen.findByRole("table", { name: "Relations of Home VPN" })).toBeVisible();
  expect(callsTo(FACTS)[0]?.init.params?.query).toMatchObject({ subject: `entity:${vpn?.id}` });
  expect(callsTo(RELATIONS)[0]?.init.params?.query).toMatchObject({
    subject: `entity:${vpn?.id}`,
  });

  await user.click(screen.getByRole("button", { name: "Delete" }));
  const dialog = await screen.findByRole("dialog", { name: "Delete Home VPN?" });
  await user.click(within(dialog).getByRole("button", { name: "Delete" }));

  await waitFor(() =>
    expect(screen.getByTestId("location")).toHaveTextContent("/knowledge?tab=entities"),
  );
  expect(callsTo(DELETE)[0]?.init.params?.query).toEqual({ expected_version: vpn?.version });
});

test("an_unknown_entity_says_no_such_entity_with_a_link_back", async () => {
  show("nothing");

  expect(await screen.findByText("No such entity")).toBeVisible();
  expect(screen.getByRole("link", { name: "Back to entities" })).toHaveAttribute(
    "href",
    "/knowledge?tab=entities",
  );
});

test("the_entity_status_chips_narrow_its_facts", async () => {
  const user = userEvent.setup();
  route(FACTS, () => ({ items: [], next_cursor: null }));
  show();
  await screen.findByRole("heading", { level: 1, name: "Home VPN" });

  await user.click(await screen.findByRole("radio", { name: "Retracted" }));

  await waitFor(() =>
    expect(callsTo(FACTS).at(-1)?.init.params?.query).toMatchObject({ status: "retracted" }),
  );
});
