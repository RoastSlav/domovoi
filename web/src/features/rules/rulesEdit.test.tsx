import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { AckRule, AckRulePatch, Host } from "../../api/types";
import { calls, resetApi, route } from "../../test/apiMock";
import { renderAt } from "../../test/render";
import { FIRST_RUN_NOTE, RulesPage } from "./RulesPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const NAS: Host = {
  id: "0196ac01-0000-7000-8000-000000000100",
  name: "nas",
  kind: "agent",
  connected: true,
  agent_enrolled: true,
  open_incidents: 0,
  outdated: false,
  protocol_state: "compatible",
  reported_protocol: "1.1",
  hub_protocol: "1.1",
  reported_release: "0.0.1",
  incompatible_since: null,
};

const LISTENER: AckRule = {
  id: "0196ac01-0000-7000-8000-000000000300",
  check: "security.new_port",
  host: "nas",
  target_pattern: "0.0.0.0:4324/tcp",
  predicates: [],
  note: FIRST_RUN_NOTE,
  expires_at: null,
};

let stored: AckRule[];

beforeEach(() => {
  resetApi();
  stored = [LISTENER];
  route("GET /api/v1/hosts", () => [NAS]);
  route("GET /api/v1/ack-rules", () => stored);
  route("PATCH /api/v1/ack-rules/{id}", (init) => {
    const patch = init.body as AckRulePatch;
    stored = stored.map((rule) => ({ ...rule, note: patch.note ?? rule.note }));
    return stored[0];
  });
});

test("rules_found_on_first_run_are_counted_in_a_banner", async () => {
  renderAt("/rules", "/rules", <RulesPage />);
  expect(await screen.findByText("Listeners found on first run")).toBeVisible();
  expect(screen.getByText(/1 known conditions were made from what an agent found/)).toBeVisible();
});

test("a_rule_can_be_edited_and_the_hub_gets_only_what_the_form_sends", async () => {
  const user = userEvent.setup();
  renderAt("/rules", "/rules", <RulesPage />);
  await screen.findByText("Listeners found on first run");

  await user.click(screen.getByRole("button", { name: "Actions for security.new_port on nas" }));
  await user.click(screen.getByRole("menuitem", { name: "Edit" }));
  const dialog = screen.getByRole("dialog");
  const note = within(dialog).getByLabelText(/Note/);
  await user.clear(note);
  await user.type(note, "the media server");
  await user.click(within(dialog).getByRole("button", { name: "Save" }));

  expect(await screen.findByText("Known condition saved")).toBeVisible();
  const sent = calls.find((call) => call.route === "PATCH /api/v1/ack-rules/{id}");
  expect(sent?.init.body).toEqual({
    host: "nas",
    target_pattern: "0.0.0.0:4324/tcp",
    note: "the media server",
  });
});
