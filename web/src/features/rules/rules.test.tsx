import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { calls, resetApi, route } from "../../test/apiMock";
import { renderAt } from "../../test/render";
import type { AckRule, AckRuleInput, Host } from "../../api/types";
import { RulesPage } from "./RulesPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

/** A rule a few steps ahead of `now`, for the table to show as not expired. */
function inDays(days: number): string {
  return new Date(Date.now() + days * 86_400_000).toISOString();
}

const NAS: Host = {
  id: "0196ac01-0000-7000-8000-000000000100",
  name: "nas",
  kind: "agent",
  connected: true,
  open_incidents: 0,
  outdated: false,
};

const BACKUP: AckRule = {
  id: "0196ac01-0000-7000-8000-000000000200",
  check: "disk.usage",
  host: "nas",
  target_pattern: "/data/*",
  predicates: [{ key: "used_percent", op: "at_most", value: "90" }],
  note: "known backup window",
  expires_at: inDays(3),
};

const OLD: AckRule = {
  id: "0196ac01-0000-7000-8000-000000000201",
  check: "security.ssh_login",
  host: null,
  target_pattern: null,
  predicates: [],
  note: "expected brute force",
  expires_at: inDays(-2),
};

let stored: AckRule[];

beforeEach(() => {
  resetApi();
  stored = [BACKUP, OLD];
  route("GET /api/v1/hosts", () => [NAS]);
  route("GET /api/v1/ack-rules", () => stored);
  route("POST /api/v1/ack-rules", (init) => {
    const body = init.body as AckRuleInput;
    const created: AckRule = {
      id: "0196ac01-0000-7000-8000-000000000202",
      check: body.check,
      host: body.host ?? null,
      target_pattern: body.target_pattern ?? null,
      predicates: body.predicates,
      note: body.note,
      expires_at: inDays(1),
    };
    stored = [...stored, created];
    return created;
  });
  route("DELETE /api/v1/ack-rules/{id}", (init) => {
    stored = stored.filter((rule) => rule.id !== init.params?.path?.id);
    return null;
  });
});

test("lists_the_stored_known_conditions", async () => {
  renderAt("/rules", "/rules", <RulesPage />);
  expect(await screen.findByText("disk.usage")).toBeVisible();
  expect(screen.getAllByText("nas").length).toBeGreaterThanOrEqual(1);
  expect(screen.getByText("/data/*")).toBeVisible();
  expect(screen.getByText("used_percent is at most 90")).toBeVisible();
  expect(screen.getByText("known backup window")).toBeVisible();
  expect(screen.getByText("in 3 days")).toBeVisible();
  expect(screen.getByText("any host")).toBeVisible();
  expect(screen.getByText("security.ssh_login")).toBeVisible();
  expect(screen.getByText("every signal")).toBeVisible();
  expect(screen.getByText("expired")).toBeVisible();
  expect(screen.getByRole("heading", { level: 1, name: "Known conditions" })).toBeVisible();
  expect(
    screen.getByText(
      "Incidents that match a known condition are kept in the history but never alert you and are not triaged.",
    ),
  ).toBeVisible();
});

test("adds_a_known_condition_from_the_form", async () => {
  const user = userEvent.setup();
  renderAt("/rules", "/rules", <RulesPage />);
  await screen.findByText("disk.usage");

  await user.type(screen.getByLabelText("Check key"), "ssh_guard");
  await user.selectOptions(screen.getByLabelText("Host"), "nas");
  await user.type(screen.getByLabelText("Target pattern"), "/var/log/auth.log");
  await user.type(screen.getByLabelText("Note"), "known brute force");
  await user.selectOptions(screen.getByLabelText("Expires in"), "1440");
  await user.click(screen.getByRole("button", { name: "Add condition" }));
  await user.type(screen.getByLabelText("Condition 1 key"), "used_percent");
  await user.selectOptions(screen.getByLabelText("Condition 1 operator"), "at_most");
  await user.type(screen.getByLabelText("Condition 1 value"), "90");
  await user.click(screen.getByRole("button", { name: "Add" }));

  expect(await screen.findByText("ssh_guard")).toBeVisible();
  const post = calls.find((call) => call.route === "POST /api/v1/ack-rules");
  expect(post?.init.body).toEqual({
    check: "ssh_guard",
    host: "nas",
    target_pattern: "/var/log/auth.log",
    note: "known brute force",
    expires_in_minutes: 1440,
    predicates: [{ key: "used_percent", op: "at_most", value: "90" }],
  });
});

test("deletes_a_rule_after_confirming", async () => {
  const user = userEvent.setup();
  renderAt("/rules", "/rules", <RulesPage />);
  await screen.findByText("disk.usage");

  await user.click(screen.getByRole("button", { name: "Delete the known condition disk.usage" }));
  const dialog = screen.getByRole("dialog");
  expect(within(dialog).getByText(/known backup window/)).toBeVisible();
  await user.click(within(dialog).getByRole("button", { name: "Delete" }));

  expect(await screen.findByText("security.ssh_login")).toBeVisible();
  expect(screen.queryByText("disk.usage")).not.toBeInTheDocument();
  const del = calls.find((call) => call.route === "DELETE /api/v1/ack-rules/{id}");
  expect(del?.init.params?.path).toEqual({ id: BACKUP.id });
});

test("asks_before_deleting", async () => {
  const user = userEvent.setup();
  renderAt("/rules", "/rules", <RulesPage />);
  await screen.findByText("disk.usage");

  await user.click(screen.getByRole("button", { name: "Delete the known condition disk.usage" }));
  await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Cancel" }));

  expect(screen.getByText("disk.usage")).toBeVisible();
  expect(calls.some((call) => call.route === "DELETE /api/v1/ack-rules/{id}")).toBe(false);
});
