import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { calls, resetApi, route } from "../../test/apiMock";
import { renderAt } from "../../test/render";
import { DockerPanel } from "./DockerPanel";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const found = {
  connected: true,
  reported: true,
  engine: "Podman",
  endpoint: "unix:///run/podman/podman.sock",
  available: true,
  locked: false,
  enabled: true,
  choice: null,
  problem: null,
};

beforeEach(() => {
  resetApi();
  route("PUT /api/v1/hosts/{name}/docker", () => ({ delivered: true }));
});

function show() {
  renderAt("/hosts/nas", "/hosts/:name", <DockerPanel host="nas" />);
}

test("docker_panel_names_the_engine_it_found_and_switches_it_off_from_the_hub", async () => {
  route("GET /api/v1/hosts/{name}/docker", () => found);
  const user = userEvent.setup();
  show();
  expect(
    await screen.findByText(/Podman found at unix:\/\/\/run\/podman\/podman.sock/),
  ).toBeVisible();
  await user.selectOptions(screen.getByLabelText("Containers"), "off");
  expect(await screen.findByText("Sent to agent.")).toBeVisible();
  const put = calls.find((c) => c.route === "PUT /api/v1/hosts/{name}/docker");
  expect(put?.init.body).toEqual({ enabled: false });
});

test("docker_panel_is_read_only_when_agent_toml_fixes_the_setting", async () => {
  route("GET /api/v1/hosts/{name}/docker", () => ({ ...found, locked: true }));
  show();
  expect(await screen.findByText(/Fixed in agent.toml: on/)).toBeVisible();
  expect(screen.getByLabelText("Containers")).toBeDisabled();
});

test("docker_panel_explains_why_no_engine_can_be_used", async () => {
  route("GET /api/v1/hosts/{name}/docker", () => ({
    ...found,
    engine: "",
    endpoint: "",
    available: false,
    enabled: false,
    problem: "no Docker or Podman socket was found",
  }));
  show();
  expect(await screen.findByText(/no Docker or Podman socket was found/)).toBeVisible();
  expect(screen.getByRole("option", { name: "Always on" })).toBeDisabled();
});

test("docker_panel_says_when_the_agent_is_offline", async () => {
  route("GET /api/v1/hosts/{name}/docker", () => ({
    ...found,
    connected: false,
    reported: false,
    engine: "",
    endpoint: "",
    available: false,
    enabled: false,
  }));
  show();
  expect(await screen.findByText(/The agent is offline/)).toBeVisible();
});
