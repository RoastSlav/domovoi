import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { failure, resetApi, route } from "../../test/apiMock";
import { ActionPicker } from "./ActionPicker";
import { Harness, entry, serveHosts, show } from "./pickerFixtures";
import { TargetPicker } from "./TargetPicker";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const restart = entry("container.restart", "Restart a container", "contain");
const logs = entry("container.logs", "Read container logs", "read");

const formOf = () => JSON.parse(screen.getByTestId("form").textContent ?? "{}");

function actions(initial = {}) {
  show(
    <Harness initial={initial}>
      {(form, change) => <ActionPicker form={form} onChange={change} />}
    </Harness>,
  );
}

beforeEach(() => resetApi());

test("the_action_picker_offers_actions_groups_and_kinds_from_the_hosts_inventory", async () => {
  serveHosts({ alpha: [restart, logs], beta: [restart] });
  actions();
  const select = await screen.findByRole("combobox", { name: /Applies to/ });
  await within(select).findByRole("option", { name: /Restart a container/ });
  const names = within(select)
    .getAllByRole("option")
    .map((o) => o.textContent);
  expect(names).toContain("Anything (any action)");
  expect(names).toContain("Read: everything that only reads");
  expect(names).toContain("Container: lifecycle (2 actions)");
  expect(names).toContain("Read container logs (container.logs)");
});

test("choosing_a_group_sets_a_category_capability_and_choosing_an_action_sets_a_contract", async () => {
  const user = userEvent.setup();
  serveHosts({ alpha: [restart, logs] });
  actions();
  const select = await screen.findByRole("combobox", { name: /Applies to/ });
  await within(select).findByRole("option", { name: /Container: lifecycle/ });
  await user.selectOptions(select, "Container: lifecycle (2 actions)");
  expect(formOf()).toMatchObject({ capKind: "category", capValue: "container.lifecycle" });
  await user.selectOptions(select, "Restart a container (container.restart)");
  expect(formOf()).toMatchObject({ capKind: "contract", capValue: "container.restart" });
  await user.selectOptions(select, "Read: everything that only reads");
  expect(formOf()).toMatchObject({ capKind: "effect", capValue: "read" });
});

test("a_value_not_in_the_inventory_is_kept_and_labelled", async () => {
  serveHosts({ alpha: [logs] });
  actions({ capKind: "contract", capValue: "container.restart" });
  expect(
    await screen.findByRole("option", { name: "Not listed on this host: container.restart" }),
  ).toBeInTheDocument();
  expect(formOf()).toMatchObject({ capKind: "contract", capValue: "container.restart" });
});

test("the_fallback_lets_a_person_type_a_name_that_is_not_listed", async () => {
  const user = userEvent.setup();
  serveHosts({ alpha: [logs] });
  actions();
  await screen.findByRole("option", { name: /Read container logs/ });
  await user.click(screen.getByRole("button", { name: /Type a name that is not listed/ }));
  await user.type(screen.getByRole("textbox", { name: "Name" }), "skill.backup_db");
  expect(formOf()).toMatchObject({ capKind: "contract", capValue: "skill.backup_db" });
});

test("a_failed_inventory_load_says_so_and_offers_the_fallback", async () => {
  route("GET /api/v1/hosts", () => [{ id: "host-a", name: "alpha" }]);
  route("GET /api/v1/hosts/{name}/capabilities", () => failure(500, "internal", "Broken."));
  actions();
  expect(await screen.findByText("This host's actions could not be loaded.")).toBeVisible();
  await waitFor(() => expect(screen.getByRole("textbox", { name: "Name" })).toBeVisible());
});

test("the_target_picker_lists_known_targets_and_adds_a_new_name_once", async () => {
  const user = userEvent.setup();
  serveHosts({ alpha: [restart] }, { alpha: ["plex", "sonarr"] });
  show(
    <Harness initial={{ targetsKind: "exact" }}>
      {(form, change) => <TargetPicker form={form} onChange={change} />}
    </Harness>,
  );
  await user.click(await screen.findByRole("checkbox", { name: "plex" }));
  expect(formOf().targetNames).toBe("plex");
  expect(screen.getByRole("checkbox", { name: "sonarr" })).not.toBeChecked();
  const add = screen.getByRole("textbox", { name: /Add another name/ });
  await user.type(add, "radarr");
  await user.click(screen.getByRole("button", { name: "Add" }));
  expect(formOf().targetNames).toBe("plex\nradarr");
  await user.type(add, "radarr");
  await user.click(screen.getByRole("button", { name: "Add" }));
  expect(formOf().targetNames).toBe("plex\nradarr");
  expect(screen.getByText("That name is already in the list.")).toBeVisible();
});
