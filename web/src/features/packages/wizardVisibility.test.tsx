import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { resetApi, route } from "../../test/apiMock";
import { nas } from "../../test/fixtures";
import { expectP0Everywhere, renderWithProfile } from "../../test/visibility";
import type { ProfileId } from "../../design/visibility";
import { InstallJobPage } from "./InstallJobPage";
import { InstallWizardPage } from "./InstallWizardPage";
import { DETAIL, STATUS, job, plan, target } from "./jobFixtures";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const WIZARD = "/capabilities/install?package=nginx-tools&version=1.2.0&host=nas";

beforeEach(() => {
  resetApi();
  route("GET /api/v1/hosts", () => [nas]);
  route("GET /api/v1/packages/status", () => STATUS);
  route("GET /api/v1/packages", () => [DETAIL]);
  route("GET /api/v1/packages/{name}/{version}", () => DETAIL);
  route("POST /api/v1/packages/jobs/plan", () => plan());
  route("GET /api/v1/packages/jobs/{id}", () =>
    job(
      [
        target({ id: "a", status: "failed", failure: { code: "transfer_failed", message: "" } }),
        target({ id: "b", host_id: "h2", host_name: "attic", status: "outcome_unknown" }),
        target({ id: "c", attempt: 2, status: "queued" }),
      ],
      { status: "partial" },
    ),
  );
});

async function review(profile: ProfileId) {
  const view = renderWithProfile(profile, WIZARD, "/capabilities/install", <InstallWizardPage />);
  await userEvent.setup().click(await screen.findByRole("button", { name: "Next" }));
  await screen.findByText(/^Plan [0-9a-f]{8}$/);
  return view;
}

async function jobPage(profile: ProfileId) {
  const view = renderWithProfile(
    profile,
    "/capabilities/jobs/job-1",
    "/capabilities/jobs/:id",
    <InstallJobPage />,
  );
  await screen.findByText("Result unknown");
  return view;
}

test("p0_visible_in_every_profile_on_the_wizard_review", async () => {
  await expectP0Everywhere({
    render: review,
    expectedIds: [
      "install.review.summary",
      "install.review.host",
      "install.review.plan",
      "install.review.notes",
      "install.review.confirm",
    ],
  });
});

test("p0_visible_in_every_profile_on_the_job_page", async () => {
  await expectP0Everywhere({
    render: jobPage,
    expectedIds: ["job.status", "job.target.status", "job.target.actions"],
  });
});

test("categories_match_the_table_for_the_wizard_and_job_page", async () => {
  const expected: [string, string][] = [
    ["install.review.summary", "P0"],
    ["install.review.host", "P0"],
    ["install.review.plan", "P0"],
    ["install.review.notes", "P0"],
    ["install.review.fingerprint", "P2"],
    ["install.review.confirm", "P0"],
  ];
  const wizard = await review("detailed");
  for (const [id, category] of expected) {
    expect(document.querySelector(`[data-visibility-id="${id}"]`)).toHaveAttribute(
      "data-visibility",
      category,
    );
  }
  wizard.unmount();
  const job = await jobPage("detailed");
  for (const [id, category] of [
    ["job.status", "P0"],
    ["job.target.row", "P1"],
    ["job.target.status", "P0"],
    ["job.target.actions", "P0"],
    ["job.target.history", "P2"],
    ["job.details", "P3"],
  ]) {
    expect(document.querySelector(`[data-visibility-id="${id}"]`), id).toHaveAttribute(
      "data-visibility",
      category,
    );
  }
  job.unmount();
});
