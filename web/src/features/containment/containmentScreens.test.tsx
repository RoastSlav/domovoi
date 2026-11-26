import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { hubHost } from "../../test/attentionFixtures";
import { serveAttention } from "../../test/attentionRoutes";
import { block, containmentStatus, containmentStep } from "../../test/containmentFixtures";
import { nas } from "../../test/fixtures";
import { renderAt } from "../../test/render";
import { expectP0Everywhere, renderWithProfile } from "../../test/visibility";
import { AttentionPage } from "../attention/AttentionPage";
import { ContainmentHost } from "./ContainmentHost";
import { ContainmentStepCard } from "./ContainmentStepCard";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const RELEASE = "POST /api/v1/operations/{id}/steps/{step}/containment/release";
const REFRESH = "POST /api/v1/hosts/{name}/containment/refresh";
const STATUS = "GET /api/v1/hosts/{name}/containment";

beforeEach(() => {
  resetApi();
  localStorage.clear();
});

const stepCard = (step = containmentStep({ phase: "confirmed" })) => (
  <ContainmentStepCard operationId="op-1" step={step} stepId="step-exec-1" />
);

describe("containment step card", () => {
  it("the_phase_timeline_shows_apply_confirm_and_where_the_block_stands", () => {
    renderAt(
      "/",
      "/",
      stepCard(containmentStep({ phase: "confirmed", expires_at: "2099-01-01T00:00:00Z" })),
    );

    const timeline = screen.getByRole("list", { name: "Phases of the block" });
    expect(within(timeline).getByText(/Apply for 30 s/)).toBeVisible();
    expect(within(timeline).getByText(/Confirm/)).toBeVisible();
    expect(within(timeline).getByText(/Blocked until/)).toBeVisible();
    expect(screen.getByText("Blocked until")).toBeVisible();
  });

  it("existing_flows_and_the_bridge_gap_are_said_in_plain_words", () => {
    renderAt(
      "/",
      "/",
      stepCard(containmentStep({ existing_flows: "may_persist", bridge_gap: true })),
    );

    expect(screen.getByText(/may keep working\. The block stops new ones/)).toBeVisible();
    expect(screen.getByText(/bridges a network interface/)).toBeVisible();
    expect(screen.getByText(/flood from the blocked source/)).toBeVisible();
  });

  it("a_lost_block_says_the_source_may_no_longer_be_blocked", () => {
    renderAt("/", "/", stepCard(containmentStep({ phase: "lost" })));

    expect(screen.getByText("Block lost")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Release block" })).toBeNull();
  });

  it("release_requires_step_up_and_typed_source", async () => {
    const user = userEvent.setup();
    route(RELEASE, () => block({ phase: "rolled_back" }));
    renderAt("/", "/", stepCard());

    await user.click(screen.getByRole("button", { name: "Release block" }));
    const dialog = await screen.findByRole("dialog");
    const confirm = within(dialog).getByRole("button", { name: "Release block" });
    expect(confirm).toHaveAttribute("aria-disabled", "true");
    await user.type(within(dialog).getByLabelText(/Type .* to confirm/), "203.0.113.7/32");
    expect(confirm, "the password is still missing").toHaveAttribute("aria-disabled", "true");
    await user.type(within(dialog).getByLabelText("Your password"), "pw");
    await user.click(confirm);

    await waitFor(() => expect(calls.filter((call) => call.route === RELEASE)).toHaveLength(1));
    expect(calls.find((call) => call.route === RELEASE)?.init.body).toEqual({
      password: "pw",
      confirm_source: "203.0.113.7/32",
    });
  });

  it("p0_visible_in_every_profile_for_the_phase_and_the_unverified_note", async () => {
    await expectP0Everywhere({
      expectedIds: ["containment.phase", "containment.unverified"],
      render: async (profile) => {
        const view = renderWithProfile(
          profile,
          "/",
          "/",
          stepCard(
            containmentStep({
              phase: "confirmed",
              assurance: "unverified",
              reason: "key_mismatch",
            }),
          ),
        );
        await screen.findByText("Coverage cannot be verified");
        return view;
      },
    });
  });
});

describe("containment on a host", () => {
  it("the_host_block_shows_the_gate_the_drills_and_each_block", async () => {
    route(STATUS, () =>
      containmentStatus({
        gate: { state: "open" },
        drills: { state: "passed" },
        blocks: [block({ phase: "confirmed" })],
        existing_flows: "cut",
      }),
    );
    renderAt("/", "/", <ContainmentHost host="nas" />);

    expect(await screen.findByText("Blocking allowed")).toBeVisible();
    expect(screen.getByText("Drills passed")).toBeVisible();
    const list = screen.getByRole("list", { name: "Blocks on nas" });
    expect(within(list).getByText("203.0.113.7/32")).toBeVisible();
    expect(within(list).getByRole("link", { name: "Open operation" })).toHaveAttribute(
      "href",
      "/operations/op-1",
    );
  });

  it("a_host_where_blocking_is_unavailable_still_shows_and_disables_refresh_with_a_reason", async () => {
    route(STATUS, () => containmentStatus({ available: false }));
    renderAt("/", "/", <ContainmentHost host="nas" />);

    expect(await screen.findByText(/has not asked this host/)).toBeVisible();
    expect(screen.getByText("No blocks recorded on this host.")).toBeVisible();
    expect(screen.getByRole("button", { name: "Refresh blocks" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
  });

  it("a_status_that_cannot_load_is_an_error_not_an_empty_list", async () => {
    route(STATUS, () => failure(500, "internal", "the hub is sick"));
    renderAt("/", "/", <ContainmentHost host="nas" />);

    expect(await screen.findByRole("button", { name: "Retry" })).toBeVisible();
    expect(screen.queryByText("No blocks recorded on this host.")).toBeNull();
  });

  it("refresh_asks_the_helper_and_shows_the_answer", async () => {
    const user = userEvent.setup();
    route(STATUS, () => containmentStatus({ gate: { state: "unknown" } }));
    route(REFRESH, () => containmentStatus({ gate: { state: "blocked_leak" } }));
    renderAt("/", "/", <ContainmentHost host="nas" />);

    await user.click(await screen.findByRole("button", { name: "Refresh blocks" }));

    expect(await screen.findByText("Blocking off: leak")).toBeVisible();
  });

  it("p0_visible_in_every_profile_for_the_gate", async () => {
    route(STATUS, () => containmentStatus({ gate: { state: "blocked_untested" } }));
    await expectP0Everywhere({
      expectedIds: ["containment.gate"],
      render: async (profile) => {
        const view = renderWithProfile(profile, "/", "/", <ContainmentHost host="nas" />);
        await screen.findByText("Blocking off: untested");
        return view;
      },
    });
  });
});

describe("containment in attention", () => {
  it("a_lost_block_is_an_act_now_item_that_leads_to_the_helper_tab", async () => {
    serveAttention({
      hosts: [nas],
      containment: { nas: containmentStatus({ blocks: [block({ phase: "lost" })] }) },
    });
    renderAt("/attention", "/attention", <AttentionPage />);

    const act = within(await screen.findByRole("region", { name: "Act now" }));
    const link = act.getByRole("link", { name: /Blocks lost on nas/ });
    expect(link).toHaveAttribute("href", "/capabilities?tab=helper");
    expect(link).toHaveTextContent("Block lost");
  });

  it("a_gate_that_refuses_to_block_is_worth_a_look", async () => {
    serveAttention({
      hosts: [nas],
      containment: { nas: containmentStatus({ gate: { state: "blocked_leak" } }) },
    });
    renderAt("/attention", "/attention", <AttentionPage />);

    const look = within(await screen.findByRole("region", { name: "Worth a look" }));
    expect(look.getByRole("link", { name: /Blocking is off on nas/ })).toBeVisible();
  });

  it("an_open_or_unasked_gate_and_a_hub_only_host_raise_nothing", async () => {
    serveAttention({
      hosts: [nas, hubHost("home")],
      containment: { nas: containmentStatus({ gate: { state: "open" } }) },
    });
    renderAt("/attention", "/attention", <AttentionPage />);

    expect(await screen.findByText("Nothing needs you")).toBeVisible();
    expect(
      calls.filter((call) => call.route === STATUS).map((call) => call.init.params?.path?.name),
    ).toEqual(["nas"]);
  });

  it("a_containment_source_that_failed_is_named_not_read_as_nothing_needs_you", async () => {
    serveAttention({ hosts: [nas] });
    route(STATUS, () => failure(500, "internal", "the hub is sick"));
    renderAt("/attention", "/attention", <AttentionPage />);

    expect(await screen.findByText(/Could not load blocks/)).toBeVisible();
    expect(screen.queryByText("Nothing needs you")).toBeNull();
  });
});
