import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import type { Execution } from "../../api/types";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { action } from "../../test/fixtures";
import { execution, PLAN_HASH } from "../../test/operationFixtures";
import { renderAt } from "../../test/render";
import { ApprovalCard, type ApprovalCardProps } from "./ApprovalCard";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const APPROVE = "POST /api/v1/operations/{id}/steps/{step}/approve";
const DENY = "POST /api/v1/operations/{id}/steps/{step}/deny";

function show(exec: Execution, props: Partial<ApprovalCardProps> = {}) {
  return renderAt(
    "/",
    "/",
    <ApprovalCard
      source={{ kind: "step", operationId: "op-1", execution: exec }}
      host="nas"
      {...props}
    />,
  );
}

function sent(name: string) {
  return calls.filter((call) => call.route === name);
}

beforeEach(resetApi);

describe("step approval card", () => {
  it("step_card_shows_skill_host_check_target_args_risk_expiry_and_the_v2_plan_prefix", () => {
    show(execution());
    const card = screen.getByRole("group", { name: "Action restart_container" });
    expect(within(card).getByText("on nas")).toBeVisible();
    expect(within(card).getByText("Check container.down")).toBeVisible();
    expect(within(card).getByText("Target plex")).toBeVisible();
    expect(within(card).getByText("name = plex")).toBeVisible();
    expect(within(card).getByText(/Expires in/)).toBeVisible();
    expect(within(card).getByText(`Plan ${PLAN_HASH.slice(0, 8)}`)).toBeVisible();
    expect(within(card).getByText("v2")).toBeVisible();
  });

  it("step_approve_asks_first_and_sends_the_plan_hash_it_showed", async () => {
    const user = userEvent.setup();
    route(APPROVE, () => ({}));
    show(execution());
    await user.click(screen.getByRole("button", { name: "Approve" }));
    const dialog = within(screen.getByRole("dialog", { name: "Run restart_container?" }));
    expect(
      dialog.getByText("restart_container with name=plex runs on nas as soon as you approve."),
    ).toBeVisible();
    expect(sent(APPROVE)).toHaveLength(0);
    await user.click(dialog.getByRole("button", { name: "Approve" }));
    expect(sent(APPROVE)).toEqual([
      {
        route: APPROVE,
        init: {
          params: { path: { id: "op-1", step: "step-exec-1" } },
          body: { plan_hash: PLAN_HASH },
        },
      },
    ]);
    expect(await screen.findByText("Approved restart_container. It runs now.")).toBeVisible();
  });

  it("step_needing_confirmation_requires_the_skill_name_typed_exactly", async () => {
    const user = userEvent.setup();
    route(APPROVE, () => ({}));
    show(execution({ risk: "destructive", confirm_required: true }));
    await user.click(screen.getByRole("button", { name: "Approve" }));
    const dialog = within(screen.getByRole("dialog"));
    const confirm = dialog.getByRole("button", { name: "Approve" });
    const field = dialog.getByRole("textbox");
    for (const wrong of ["Restart_container", "restart_container ", "restart"]) {
      await user.clear(field);
      await user.type(field, wrong);
      expect(confirm).toHaveAttribute("aria-disabled", "true");
    }
    await user.clear(field);
    await user.type(field, "restart_container");
    expect(confirm).not.toHaveAttribute("aria-disabled", "true");
    await user.click(confirm);
    expect(sent(APPROVE)[0]?.init.body).toEqual({
      plan_hash: PLAN_HASH,
      confirm_text: "restart_container",
    });
  });

  it("an_unknown_risk_step_is_treated_as_destructive", async () => {
    const user = userEvent.setup();
    show(execution({ risk: null, confirm_required: true }));
    expect(screen.getByText("Risk unknown")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Approve" }));
    const dialog = within(screen.getByRole("dialog"));
    expect(dialog.getByRole("textbox")).toBeVisible();
    expect(dialog.getByText(/Its risk is unknown, so it is treated as destructive/)).toBeVisible();
  });

  it("a_missing_confirm_required_field_is_treated_as_required", async () => {
    const user = userEvent.setup();
    const exec = { ...execution() } as Partial<Execution>;
    delete exec.confirm_required;
    show(exec as Execution);
    await user.click(screen.getByRole("button", { name: "Approve" }));
    expect(within(screen.getByRole("dialog")).getByRole("textbox")).toBeVisible();
  });

  it("the_hash_sent_is_the_one_shown_when_the_dialog_opened", async () => {
    const user = userEvent.setup();
    route(APPROVE, () => ({}));
    function Swap() {
      const [changed, setChanged] = useState(false);
      return (
        <>
          <button type="button" onClick={() => setChanged(true)}>
            Change plan
          </button>
          <ApprovalCard
            source={{
              kind: "step",
              operationId: "op-1",
              execution: execution({ plan_hash: changed ? "cd".repeat(32) : PLAN_HASH }),
            }}
          />
        </>
      );
    }
    renderAt("/", "/", <Swap />);
    await user.click(screen.getByRole("button", { name: "Approve" }));
    await user.click(screen.getByRole("button", { name: "Change plan" }));
    const dialog = within(screen.getByRole("dialog"));
    expect(dialog.getByRole("button", { name: "Approve" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    expect(dialog.getAllByText(/The plan changed while you were looking/).length).toBeGreaterThan(
      0,
    );
    expect(sent(APPROVE)).toHaveLength(0);
  });

  it("approve_conflict_is_shown_inside_the_dialog_in_the_hubs_words", async () => {
    const user = userEvent.setup();
    route(APPROVE, () =>
      failure(409, "conflict", "the plan changed since you opened it; reload the page"),
    );
    show(execution());
    await user.click(screen.getByRole("button", { name: "Approve" }));
    const dialog = within(screen.getByRole("dialog"));
    await user.click(dialog.getByRole("button", { name: "Approve" }));
    expect(await dialog.findByText("Could not approve the action")).toBeVisible();
    expect(screen.getByRole("dialog")).toBeVisible();
  });

  it("expired_step_disables_approve_with_a_reason_and_keeps_deny", () => {
    show(execution({ expires_at: "2020-01-01T00:00:00Z" }));
    expect(screen.getByRole("button", { name: "Approve" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
    expect(screen.getByRole("button", { name: "Deny" })).not.toHaveAttribute(
      "aria-disabled",
      "true",
    );
  });

  it("deny_is_one_click_and_says_nothing_ran", async () => {
    const user = userEvent.setup();
    route(DENY, () => ({}));
    show(execution());
    await user.click(screen.getByRole("button", { name: "Deny" }));
    expect(sent(DENY)).toHaveLength(1);
    expect(sent(APPROVE)).toHaveLength(0);
    expect(await screen.findByText("Denied restart_container. Nothing ran.")).toBeVisible();
  });

  it("a_decided_step_shows_who_approved_it_and_has_no_buttons", () => {
    show(
      execution({
        state: "succeeded",
        approved_by: "user:1",
        approved_at: "2026-10-31T12:00:00Z",
      }),
    );
    expect(screen.getByText(/Approved by user:1/)).toBeVisible();
    expect(screen.queryByRole("button", { name: "Approve" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Deny" })).toBeNull();
  });

  it("read_only_card_has_no_decision_buttons", () => {
    show(execution(), { readOnly: true });
    expect(screen.queryByRole("button", { name: "Approve" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Deny" })).toBeNull();
  });

  it.each(["refused", "not_run", "unavailable"])(
    "refused_not_run_and_unavailable_steps_explain_themselves (%s)",
    (state) => {
      show(execution({ state, reason: "because of reasons" }));
      expect(screen.getByText("because of reasons")).toBeVisible();
    },
  );

  it.each(["authorized", "dispatched"])("in_flight_step_shows_progress (%s)", (state) => {
    show(execution({ state }));
    expect(screen.getByRole("progressbar")).toBeVisible();
  });

  it("model_text_is_secondary_and_labelled_and_rendered_as_text", () => {
    show(execution(), { why: "<b>x</b>" });
    expect(screen.getByText("<b>x</b>", { exact: false })).toBeVisible();
    expect(screen.getByText("Model's reason (not checked):", { exact: false })).toBeVisible();
    expect(document.querySelector("b")).toBeNull();
  });

  it("extras_render_only_when_given", () => {
    const first = show(execution());
    expect(screen.queryByText(/Effect:/)).toBeNull();
    expect(screen.queryByText(/Policy:/)).toBeNull();
    expect(screen.queryByText("The agent refused it")).toBeNull();
    first.unmount();
    show(execution(), {
      extras: {
        effect: { class: "service restart", reversible: true },
        policy: { outcome: "needs approval", rule: "r1" },
        refusal: { reason: "No such container" },
      },
    });
    expect(screen.getByText("Effect: service restart")).toBeVisible();
    expect(screen.getByText("Reversible")).toBeVisible();
    expect(screen.getByText("Policy: needs approval (rule r1)")).toBeVisible();
    expect(screen.getByText("The agent refused it")).toBeVisible();
  });

  it("legacy_card_keeps_its_output_and_v1_label", () => {
    renderAt(
      "/",
      "/",
      <ApprovalCard source={{ kind: "legacy", action: action({ plan_binding: "v1" }) }} />,
    );
    expect(screen.getByText("v1 binding")).toBeVisible();
  });

  it("a_card_with_an_operation_link_links_to_the_operation", () => {
    show(execution(), { operation: { id: "op-1", goal: "why plex is down" } });
    expect(screen.getByRole("link", { name: "why plex is down" })).toHaveAttribute(
      "href",
      "/operations/op-1",
    );
  });
});
