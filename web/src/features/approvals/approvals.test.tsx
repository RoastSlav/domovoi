import { screen } from "@testing-library/react";
import { resetApi, route } from "../../test/apiMock";
import { detail, nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { ApprovalsPage } from "./ApprovalsPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

beforeEach(() => {
  resetApi();
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/incidents/{id}", () => detail());
});

test("approvals_page_explains_that_nothing_runs_until_approved", async () => {
  renderAt("/approvals", "/approvals", <ApprovalsPage />);

  expect(await screen.findByText(/^Nothing runs until you approve it\./)).toBeVisible();
  expect(
    screen.getByRole("link", { name: "Learn more about approvals (opens in a new tab)" }),
  ).toHaveAttribute(
    "href",
    "https://RoastSlav.github.io/domovoi/concepts/operations.html#approval",
  );
  expect(await screen.findByText("Approve on the operation")).toBeVisible();
  expect(screen.getByRole("link", { name: "operations" })).toHaveAttribute("href", "/operations");
  expect(screen.queryByRole("button", { name: "Approve" }), "no action can be approved").toBeNull();
});
