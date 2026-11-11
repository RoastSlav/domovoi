import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { renderAt } from "../../test/render";
import { HowToUpdateDialog } from "./HowToUpdateDialog";
import { UpdateAbout } from "./UpdateAbout";
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
  expect(await screen.findByText("Domovoi v0.2.0 is available")).toBeVisible();
  expect(screen.getByText("This hub runs 0.1.0.")).toBeVisible();
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

test("whats_new_shows_the_notes_and_links_to_the_release", async () => {
  const user = userEvent.setup();
  renderBanner();
  await user.click(await screen.findByRole("button", { name: "What's new" }));
  const dialog = screen.getByRole("dialog", { name: "What's new in Domovoi v0.2.0" });
  expect(await within(dialog).findByText("Fixed")).toBeVisible();
  expect(within(dialog).getByRole("link", { name: "View the release on GitHub" })).toHaveAttribute(
    "href",
    update.url,
  );
});

test("about_shows_the_version_and_offers_how_to_update_when_a_release_is_newer", async () => {
  const user = userEvent.setup();
  renderAt("/settings", "/settings", <UpdateAbout />);
  expect(await screen.findByText("Domovoi v0.2.0 is available")).toBeVisible();
  expect(screen.getByText("0.1.0")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "How to update" }));
  expect(screen.getByRole("dialog", { name: "How to update Domovoi" })).toBeVisible();
});

test("about_checks_now_and_says_when_nothing_is_newer", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/system/update", () => ({ current: "0.2.0", latest: "v0.2.0", newer: false }));
  route("POST /api/v1/system/update/check", () => ({
    current: "0.2.0",
    latest: "v0.2.0",
    newer: false,
  }));
  renderAt("/settings", "/settings", <UpdateAbout />);
  expect(await screen.findByText("Up to date")).toBeVisible();
  expect(screen.queryByRole("button", { name: "How to update" })).toBeNull();
  await user.click(screen.getByRole("button", { name: "Check now" }));
  expect(await screen.findByText("You are on the latest version")).toBeVisible();
  expect(calls.some((c) => c.route === "POST /api/v1/system/update/check")).toBe(true);
});

test("about_says_when_the_release_check_cannot_load", async () => {
  route("GET /api/v1/system/update", () => failure(503, "unavailable", "no answer"));
  renderAt("/settings", "/settings", <UpdateAbout />);
  expect(await screen.findByText("Could not load the release check")).toBeVisible();
  expect(screen.getByRole("button", { name: "Retry" })).toBeVisible();
});

test("how_to_update_without_a_checked_release_says_to_check_first", async () => {
  route("GET /api/v1/system/update", () => ({ current: "0.1.0", latest: null, newer: false }));
  renderAt("/hosts", "/hosts", <HowToUpdateDialog onClose={() => undefined} />);
  expect(await screen.findByText("No release has been checked yet")).toBeVisible();
});
