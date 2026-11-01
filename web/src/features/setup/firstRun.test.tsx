import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { calls, resetApi, route } from "../../test/apiMock";
import { renderAt } from "../../test/render";
import { FirstRun } from "./FirstRun";

vi.mock("../../api/client", () => import("../../test/apiMock"));
vi.mock("../../api/health", () => ({ waitForHealthy: () => Promise.resolve(true) }));

beforeEach(() => {
  resetApi();
  route("GET /api/v1/hosts", () => []);
  route("GET /api/v1/settings", () => ({
    editable: {
      revision: 0,
      values: { hub: { name: "domovoi" } },
      fields: {},
      issues: [],
      last_apply: null,
    },
  }));
  route("PATCH /api/v1/settings", () => ({ revision: 1, applied: true }));
});

test("first_run_stepper_skips_optional_steps_and_applies_once", async () => {
  const user = userEvent.setup();
  renderAt("/welcome", "/welcome", <FirstRun />);
  expect(await screen.findByRole("heading", { name: "Hub" })).toBeVisible();
  expect(screen.queryByRole("button", { name: "Skip" })).toBeNull();
  expect(screen.getByLabelText("Public URL")).toHaveValue(window.location.origin);
  await user.clear(screen.getByLabelText("Name"));
  await user.type(screen.getByLabelText("Name"), "home");
  await user.click(screen.getByRole("button", { name: "Next" }));

  expect(screen.getByRole("heading", { name: "Discord" })).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Set Bot token" }));
  await user.type(screen.getByLabelText("New Bot token"), "typed-then-skipped");
  await user.click(screen.getByRole("button", { name: "Skip" }));
  expect(screen.getByRole("heading", { name: "Model" })).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Skip" }));
  expect(screen.getByRole("heading", { name: "Digest and quiet hours" })).toBeVisible();
  await user.type(screen.getByLabelText("Digest time"), "08:30");
  expect(calls.filter((c) => c.route === "PATCH /api/v1/settings")).toHaveLength(0);
  await user.click(screen.getByRole("button", { name: "Save and apply" }));

  expect(await screen.findByRole("heading", { name: "Add your first host" })).toBeVisible();
  const saves = calls.filter((c) => c.route === "PATCH /api/v1/settings");
  expect(saves).toHaveLength(1);
  const body = saves[0]?.init.body as {
    base_revision: number;
    changes: Record<string, unknown>;
    password?: string;
  };
  expect(body.base_revision).toBe(0);
  expect(body.password).toBeUndefined();
  expect(body.changes).toEqual({
    hub: { name: "home", timezone: Intl.DateTimeFormat().resolvedOptions().timeZone },
    web: { public_url: window.location.origin },
    notify: { digest: { time: "08:30" } },
  });
  expect(JSON.stringify(body)).not.toContain("typed-then-skipped");
  await user.click(screen.getByRole("button", { name: "Finish" }));
  expect(await screen.findByTestId("location")).toHaveTextContent("/incidents");
});
