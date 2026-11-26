import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import {
  containmentStep,
  containmentStepDetail,
  unverifiedStep,
} from "../../test/containmentFixtures";
import { execution, PLAN_HASH } from "../../test/operationFixtures";
import { renderAt } from "../../test/render";
import { expectP0Everywhere, renderWithProfile } from "../../test/visibility";
import { ApprovalCard } from "../approvals/ApprovalCard";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const APPROVE = "POST /api/v1/operations/{id}/steps/{step}/approve";
const STEP = "GET /api/v1/operations/{id}/steps/{step}";
const SOURCE = "203.0.113.7/32";

const block = () =>
  execution({
    contract: "firewall.block_source",
    target: `ipnet:${SOURCE}`,
    args: { source: SOURCE, ttl_s: "3600" },
    risk: "write",
  });

function serveStep(containment: ReturnType<typeof containmentStep> | null) {
  route(STEP, () => containmentStepDetail(containment));
}

const card = () => (
  <ApprovalCard source={{ kind: "step", operationId: "op-1", execution: block() }} host="nas" />
);

beforeEach(resetApi);

describe("unverified human containment approval", () => {
  it("unverified_containment_card_shows_the_warning_the_reason_and_what_is_not_protected", async () => {
    serveStep(unverifiedStep());
    renderAt("/", "/", card());

    expect(await screen.findByText("Coverage cannot be verified")).toBeVisible();
    expect(screen.getByText("You have not pinned the helper's key yet.")).toBeVisible();
    const paths = screen.getByRole("list", { name: "Paths not protected" });
    expect(within(paths).getByText("ssh on 192.0.2.10")).toBeVisible();
    expect(within(paths).getByText("hub link from 198.51.100.4")).toBeVisible();
  });

  it("unverified_human_path_needs_password_typed_source_and_acknowledgement", async () => {
    const user = userEvent.setup();
    route(APPROVE, () => ({}));
    serveStep(unverifiedStep());
    renderAt("/", "/", card());

    await user.click(await screen.findByRole("button", { name: "Approve" }));
    const dialog = await screen.findByRole("dialog");
    const confirm = within(dialog).getByRole("button", { name: "Approve block" });
    expect(confirm).toHaveAttribute("aria-disabled", "true");

    await user.type(within(dialog).getByLabelText(/Type .* to confirm/), SOURCE);
    expect(confirm, "still needs the tick and the password").toHaveAttribute(
      "aria-disabled",
      "true",
    );
    await user.click(within(dialog).getByRole("checkbox"));
    expect(confirm, "still needs the password").toHaveAttribute("aria-disabled", "true");
    await user.type(within(dialog).getByLabelText("Your password"), "correct horse");
    expect(confirm).not.toHaveAttribute("aria-disabled", "true");

    await user.click(confirm);
    await waitFor(() => expect(calls.filter((call) => call.route === APPROVE)).toHaveLength(1));
    expect(calls.find((call) => call.route === APPROVE)?.init.body).toEqual({
      plan_hash: PLAN_HASH,
      password: "correct horse",
      unverified_acknowledged: true,
    });
  });

  it("a_wrong_password_is_said_in_the_dialog_and_the_password_is_cleared", async () => {
    const user = userEvent.setup();
    route(APPROVE, () => failure(403, "wrong_password", "That password is not right."));
    serveStep(unverifiedStep());
    renderAt("/", "/", card());

    await user.click(await screen.findByRole("button", { name: "Approve" }));
    const dialog = await screen.findByRole("dialog");
    await user.type(within(dialog).getByLabelText(/Type .* to confirm/), SOURCE);
    await user.click(within(dialog).getByRole("checkbox"));
    await user.type(within(dialog).getByLabelText("Your password"), "nope");
    await user.click(within(dialog).getByRole("button", { name: "Approve block" }));

    expect(await within(dialog).findByText(/That password is not right/)).toBeVisible();
    expect(within(dialog).getByLabelText("Your password")).toHaveValue("");
  });

  it("verified_containment_is_approved_like_any_other_step", async () => {
    serveStep(containmentStep());
    renderAt("/", "/", card());

    await screen.findByRole("button", { name: "Approve" });
    expect(screen.queryByText("Coverage cannot be verified")).toBeNull();
  });

  it("a_containment_step_that_cannot_load_says_so_and_offers_no_approve", async () => {
    route(STEP, () => failure(500, "internal", "the hub is sick"));
    renderAt("/", "/", card());

    expect(await screen.findByRole("button", { name: "Retry" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "Approve" })).toBeNull();
  });

  it("p0_visible_in_every_profile_for_the_unverified_card", async () => {
    serveStep(unverifiedStep());
    await expectP0Everywhere({
      expectedIds: ["containment.unverified"],
      render: async (profile) => {
        const view = renderWithProfile(profile, "/", "/", card());
        await screen.findByText("Coverage cannot be verified");
        return view;
      },
    });
  });
});
