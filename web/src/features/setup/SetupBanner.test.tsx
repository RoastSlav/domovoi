import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { resetApi, route } from "../../test/apiMock";
import { renderAt } from "../../test/render";
import { SetupBanner } from "./SetupBanner";

vi.mock("../../api/client", () => import("../../test/apiMock"));

function settings(values: Record<string, unknown>) {
  route("GET /api/v1/settings", () => ({
    editable: { revision: 0, values, fields: {}, issues: [], last_apply: null },
  }));
}

beforeEach(resetApi);

test("setup_banner_says_what_is_left_and_links_to_the_first_run", async () => {
  route("GET /api/v1/hosts", () => []);
  settings({ discord: { channel_id: "1" } });
  renderAt("/incidents", "/incidents", <SetupBanner />);
  expect(await screen.findByText("Setup is not finished")).toBeVisible();
  expect(screen.getByText("Still to add: a model and a host.")).toBeVisible();
  expect(screen.getByRole("link", { name: "Finish setup" })).toHaveAttribute("href", "/welcome");
});

test("setup_banner_stays_dismissed_and_shows_nothing_when_all_is_set_up", async () => {
  const user = userEvent.setup();
  route("GET /api/v1/hosts", () => []);
  settings({});
  const { unmount } = renderAt("/incidents", "/incidents", <SetupBanner />);
  await user.click(await screen.findByRole("button", { name: "Dismiss" }));
  expect(screen.queryByText("Setup is not finished")).toBeNull();
  unmount();

  renderAt("/incidents", "/incidents", <SetupBanner />);
  await screen.findByTestId("location");
  expect(screen.queryByText("Setup is not finished")).toBeNull();
});
