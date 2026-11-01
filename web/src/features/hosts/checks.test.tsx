import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { calls, resetApi, route } from "../../test/apiMock";
import { renderAt } from "../../test/render";
import { ChecksTab } from "./ChecksTab";

vi.mock("../../api/client", () => import("../../test/apiMock"));

beforeEach(() => {
  resetApi();
  route("GET /api/v1/hosts/{name}/checks", () => ({
    connected: false,
    checks: [
      {
        key: "disk.usage",
        reported: null,
        overrides: { enabled: null, interval_seconds: 300, thresholds: {} },
      },
    ],
  }));
  route("PUT /api/v1/hosts/{name}/checks", () => ({ delivered: false }));
});

test("host_checks_tab_saves_overrides_and_reports_offline_agent", async () => {
  const user = userEvent.setup();
  renderAt("/hosts/nas", "/hosts/:name", <ChecksTab host="nas" />);
  expect(await screen.findByText(/The agent is offline/)).toBeVisible();
  expect(screen.getByLabelText("disk.usage interval in seconds")).toHaveValue("300");
  await user.selectOptions(screen.getByLabelText("disk.usage runs"), "off");
  await user.type(screen.getByLabelText("disk.usage thresholds"), '{{"warn_percent": 70}');
  await user.click(screen.getByRole("button", { name: "Save checks" }));
  expect(await screen.findByText("Agent offline, will apply on reconnect.")).toBeVisible();
  const put = calls.find((c) => c.route === "PUT /api/v1/hosts/{name}/checks");
  expect(put?.init.params?.path).toEqual({ name: "nas" });
  expect(put?.init.body).toEqual({
    overrides: {
      "disk.usage": { enabled: false, interval_seconds: 300, thresholds: { warn_percent: 70 } },
    },
  });

  route("PUT /api/v1/hosts/{name}/checks", () => ({ delivered: true }));
  await user.click(screen.getByRole("button", { name: "Save checks" }));
  expect(await screen.findByText("Sent to agent.")).toBeVisible();
});
