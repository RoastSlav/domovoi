import { screen, within } from "@testing-library/react";
import { resetApi, route } from "../test/apiMock";
import { nas } from "../test/fixtures";
import { renderAt } from "../test/render";
import { HostPills } from "./Layout";

vi.mock("../api/client", () => import("../test/apiMock"));

beforeEach(resetApi);

/** The hub's own machine, with an agent never enrolled. */
const hubHost = {
  ...nas,
  id: "host-hub",
  name: "home-server",
  kind: "local",
  connected: false,
  agent_enrolled: false,
  version: null,
  last_seen: null,
};

/** The hub's own machine whose agent enrolled and then went offline. */
const silentHub = {
  ...hubHost,
  id: "host-silent-hub",
  name: "workshop",
  agent_enrolled: true,
};

/** The `pills` whose accessible text contains `text`. */
function pillsWith(pills: HTMLElement[], text: string) {
  return pills.filter((pill) => within(pill).queryByText(text));
}

test("nav_bar_pill_marks_the_hub_machine_as_hub_only", async () => {
  route("GET /api/v1/hosts", () => [hubHost, nas]);
  renderAt("/hosts", "/hosts", <HostPills />);

  const pills = await screen.findAllByRole("link");
  expect(pills).toHaveLength(2);

  expect(pillsWith(pills, "hub only")).toHaveLength(1);
  expect(pillsWith(pills, "connected")).toHaveLength(1);
  expect(pills.filter((pill) => pill.getAttribute("data-connected") === "hub")).toHaveLength(1);
  expect(pills.filter((pill) => pill.getAttribute("data-connected") === "true")).toHaveLength(1);
});

test("nav_bar_pill_of_an_enrolled_agent_stays_offline_not_hub_only", async () => {
  route("GET /api/v1/hosts", () => [silentHub]);
  renderAt("/hosts", "/hosts", <HostPills />);

  const pills = await screen.findAllByRole("link");
  expect(pills).toHaveLength(1);

  expect(pillsWith(pills, "hub only")).toHaveLength(0);
  expect(pillsWith(pills, "offline")).toHaveLength(1);
  expect(pills.filter((pill) => pill.getAttribute("data-connected") === "hub")).toHaveLength(0);
  expect(pills.filter((pill) => pill.getAttribute("data-connected") === "false")).toHaveLength(1);
});
