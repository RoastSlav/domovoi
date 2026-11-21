import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { failure, resetApi, route } from "../../test/apiMock";
import { ENTITIES, knowledgeEntity } from "../../test/knowledgeFixtures";
import { callsTo, lastBody, mockKnowledgeApi } from "../../test/knowledgeRoutes";
import { renderAt } from "../../test/render";
import { KnowledgePage } from "./KnowledgePage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const LIST = "GET /api/v1/knowledge/entities";
const CREATE = "POST /api/v1/knowledge/entities";
const RENAME = "PUT /api/v1/knowledge/entities/{id}";
const DELETE = "DELETE /api/v1/knowledge/entities/{id}";
const first = ENTITIES[0];

beforeEach(() => {
  resetApi();
  mockKnowledgeApi();
  route(CREATE, () => knowledgeEntity({ id: "new" }));
  route(RENAME, () => knowledgeEntity());
  route(DELETE, () => undefined);
});

function show() {
  return renderAt("/knowledge?tab=entities", "/knowledge", <KnowledgePage />);
}

test("entities_tab_lists_the_entities_and_links_each_to_its_page", async () => {
  show();

  const table = await screen.findByRole("table", { name: "Entities" });
  expect(within(table).getAllByRole("row")).toHaveLength(ENTITIES.length + 1);
  expect(within(table).getByRole("link", { name: "Home VPN" })).toHaveAttribute(
    "href",
    `/knowledge/entities/${first?.id}`,
  );
  expect(within(table).getByText("vpn")).toBeVisible();
});

test("entities_empty_and_failed_loads_are_different_things", async () => {
  route(LIST, () => []);
  const view = show();
  expect(await screen.findByText("No entities yet")).toBeVisible();
  view.unmount();

  route(LIST, () => failure(500, "internal", "boom"));
  show();
  expect(await screen.findByRole("button", { name: "Retry" })).toBeVisible();
});

test("an_entity_is_created_and_a_duplicate_name_reads_inside_the_dialog", async () => {
  const user = userEvent.setup();
  show();
  await screen.findByRole("table", { name: "Entities" });

  await user.click(screen.getByRole("button", { name: "Add an entity" }));
  const dialog = await screen.findByRole("dialog", { name: "Add an entity" });
  await user.type(within(dialog).getByLabelText("Kind"), "pool");
  await user.type(within(dialog).getByLabelText("Name"), "Fast pool");
  await user.click(within(dialog).getByRole("button", { name: "Add entity" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(lastBody(CREATE)).toEqual({ kind: "pool", name: "Fast pool" });

  route(CREATE, () => failure(409, "conflict", "an entity with that name already exists"));
  await user.click(screen.getByRole("button", { name: "Add an entity" }));
  const again = await screen.findByRole("dialog", { name: "Add an entity" });
  await user.type(within(again).getByLabelText("Kind"), "pool");
  await user.type(within(again).getByLabelText("Name"), "Fast pool");
  await user.click(within(again).getByRole("button", { name: "Add entity" }));
  expect(await within(again).findByText(/already exists/i)).toBeVisible();
});

test("an_entity_is_renamed_with_its_version_and_a_stale_rename_reads_inside_the_dialog", async () => {
  const user = userEvent.setup();
  show();
  const table = await screen.findByRole("table", { name: "Entities" });

  await user.click(within(table).getAllByRole("button", { name: "Rename" })[0] as HTMLElement);
  const dialog = await screen.findByRole("dialog", { name: "Rename Home VPN" });
  const name = within(dialog).getByLabelText("Name");
  expect(name).toHaveValue("Home VPN");
  await user.clear(name);
  await user.type(name, "Office VPN");
  await user.click(within(dialog).getByRole("button", { name: "Rename" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(lastBody(RENAME)).toEqual({ name: "Office VPN", expected_version: first?.version });

  route(RENAME, () => failure(409, "conflict", "the entity changed meanwhile; reload"));
  await user.click(within(table).getAllByRole("button", { name: "Rename" })[0] as HTMLElement);
  const stale = await screen.findByRole("dialog", { name: "Rename Home VPN" });
  await user.type(within(stale).getByLabelText("Name"), "!");
  await user.click(within(stale).getByRole("button", { name: "Rename" }));
  expect(await within(stale).findByText(/changed meanwhile/i)).toBeVisible();
});

test("an_entity_is_deleted_with_its_version_and_an_entity_in_use_reads_inside_the_dialog", async () => {
  const user = userEvent.setup();
  show();
  const table = await screen.findByRole("table", { name: "Entities" });

  await user.click(within(table).getAllByRole("button", { name: "Delete" })[0] as HTMLElement);
  const dialog = await screen.findByRole("dialog", { name: "Delete Home VPN?" });
  expect(
    within(dialog).getByText("It must not be used by any live fact or relation."),
  ).toBeVisible();
  await user.click(within(dialog).getByRole("button", { name: "Delete" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(callsTo(DELETE)[0]?.init.params?.query).toEqual({ expected_version: first?.version });

  route(DELETE, () => failure(409, "conflict", "it is still used by a live fact"));
  await user.click(within(table).getAllByRole("button", { name: "Delete" })[0] as HTMLElement);
  const busy = await screen.findByRole("dialog", { name: "Delete Home VPN?" });
  await user.click(within(busy).getByRole("button", { name: "Delete" }));
  expect(await within(busy).findByText(/still used by a live fact/i)).toBeVisible();
});
