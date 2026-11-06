import { screen } from "@testing-library/react";
import { resetApi, route } from "../../test/apiMock";
import { renderAt } from "../../test/render";
import { DigestPage } from "./DigestPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

beforeEach(() => {
  resetApi();
  route("GET /api/v1/digest/preview", () => ({
    date: "2026-11-06",
    markdown: "nas: healthy, nothing changed",
  }));
});

test("digest_page_explains_that_it_is_one_summary_per_day", async () => {
  renderAt("/digest", "/digest", <DigestPage />);

  // The page says what a digest is and what is in it.
  expect(await screen.findByText(/^One summary of every host per day:/)).toBeVisible();
  expect(screen.getByText(/what changed, trends and open incidents\./)).toBeVisible();
  expect(
    screen.getByRole("link", { name: "Learn more about the digest (opens in a new tab)" }),
  ).toHaveAttribute("href", "https://RoastSlav.github.io/domovoi/concepts/digest.html");
});
