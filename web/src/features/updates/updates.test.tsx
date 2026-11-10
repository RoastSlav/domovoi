import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { resetApi, route } from "../../test/apiMock";
import { renderAt } from "../../test/render";
import { UpdateBanner } from "./UpdateBanner";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const update = {
  current: "0.1.0",
  latest: "v0.2.0",
  newer: true,
  url: "https://github.com/RoastSlav/domovoi/releases/tag/v0.2.0",
  notes: "**Fixed** a noisy check.",
  published_at: "2026-11-09T10:00:00Z",
  checked_at: "2026-11-10T12:00:00Z",
  upgrade_command: "sudo domovoi upgrade --version v0.2.0 && sudo systemctl restart domovoi-hub",
  hub_command:
    "curl -fsSL https://github.com/RoastSlav/domovoi/releases/download/v0.2.0/install.sh | sudo sh -s -- --role hub --version v0.2.0 && sudo systemctl restart domovoi-hub",
  agent_command:
    "curl -fsSL https://github.com/RoastSlav/domovoi/releases/download/v0.2.0/install.sh | sudo sh -s -- --role agent --version v0.2.0 && sudo systemctl restart domovoi-agent",
  docker_command:
    "DOMOVOI_IMAGE_TAG=0.2.0 docker compose -f hub.yml pull && DOMOVOI_IMAGE_TAG=0.2.0 docker compose -f hub.yml up -d",
};

function renderBanner() {
  return renderAt("/incidents", "/incidents", <UpdateBanner />);
}

beforeEach(() => {
  resetApi();
  route("GET /api/v1/system/update", () => update);
});

test("banner_shows_a_newer_release", async () => {
  renderBanner();
  expect(await screen.findByText("Domovoi v0.2.0")).toBeVisible();
  expect(screen.getByText(/this hub runs 0\.1\.0/)).toBeVisible();
});

test("banner_hides_after_dismiss_and_remembers_the_version", async () => {
  const user = userEvent.setup();
  const first = renderBanner();
  await user.click(await screen.findByRole("button", { name: "Dismiss" }));
  expect(screen.queryByRole("button", { name: "Dismiss" })).toBeNull();
  expect(localStorage.getItem("update-dismissed:v0.2.0")).toBe("1");

  first.unmount();
  renderBanner();
  expect(screen.queryByRole("button", { name: "Dismiss" })).toBeNull();
});

test("how_to_update_shows_the_hub_agent_and_docker_commands", async () => {
  const user = userEvent.setup();
  renderBanner();
  await user.click(await screen.findByRole("button", { name: "How to update" }));

  expect(screen.getByRole("dialog", { name: "How to update Domovoi" })).toBeVisible();
  expect(screen.getByLabelText("Hub upgrade command")).toHaveTextContent(
    "sudo domovoi upgrade --version v0.2.0",
  );
  expect(screen.getByLabelText("Hub update command")).toHaveTextContent(
    "--role hub --version v0.2.0",
  );
  expect(screen.getByLabelText("Agent update command")).toHaveTextContent(
    "--role agent --version v0.2.0",
  );
  expect(screen.getByLabelText("Docker update command")).toHaveTextContent(
    "DOMOVOI_IMAGE_TAG=0.2.0 docker compose -f hub.yml pull",
  );
  expect(screen.getByText("Update the hub first, then the agents.")).toBeVisible();
});
