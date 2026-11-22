import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Execution } from "../../api/types";
import { PROFILE_IDS, type ProfileId } from "../../design/visibility";
import { calls, resetApi, route } from "../../test/apiMock";
import { offlineHost, summary } from "../../test/attentionFixtures";
import { serveAttention, waitingDetail } from "../../test/attentionRoutes";
import { incident, nas, overview } from "../../test/fixtures";
import { execution, PLAN_HASH } from "../../test/operationFixtures";
import { expectP0Everywhere, renderWithProfile } from "../../test/visibility";
import { OverviewPage } from "../overview/OverviewPage";
import { AttentionPage } from "./AttentionPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const SLOW = 40_000;
const APPROVE = "POST /api/v1/operations/{id}/steps/{step}/approve";

beforeEach(() => {
  resetApi();
  localStorage.clear();
  route("GET /api/v1/overview", () => overview({ attention_total: 2 }));
  serveAttention({
    hosts: [nas, offlineHost("pi")],
    incidents: [incident()],
    operations: {
      waiting_approval: [summary("op-0", "waiting_approval")],
      running: [summary("run", "running", { goal: "Fix the disk" })],
    },
    details: { "op-0": waitingDetail("op-0", "Goal op-0") },
  });
});

const controlRoom = (profile: ProfileId) => renderWithProfile(profile, "/", "/", <OverviewPage />);
const attention = (profile: ProfileId) =>
  renderWithProfile(profile, "/attention", "/attention", <AttentionPage />);

/** Waits until the sources the pages draw have answered. */
async function settled() {
  await screen.findByRole("group", { name: "Action restart_container" });
  await screen.findByRole("link", { name: /Disk \/srv is 93% full/ });
}

describe("attention visibility", () => {
  it(
    "p0_visible_in_every_profile_on_the_control_room",
    () =>
      expectP0Everywhere({
        expectedIds: [
          "control.tiles",
          "control.attention",
          "control.running",
          "control.approvals",
          "control.hosts",
        ],
        render: async (profile) => {
          const view = controlRoom(profile);
          await settled();
          await screen.findByRole("link", { name: /^Needs you 2/ });
          return view;
        },
      }),
    SLOW,
  );

  it(
    "p0_visible_in_every_profile_on_the_attention_centre",
    () =>
      expectP0Everywhere({
        expectedIds: ["attention.list", "attention.item"],
        render: async (profile) => {
          const view = attention(profile);
          await settled();
          return view;
        },
      }),
    SLOW,
  );

  it(
    "the_profile_never_changes_which_requests_these_pages_make",
    async () => {
      const requests = new Map<string, string[]>();
      for (const [name, render] of [
        ["control room", controlRoom],
        ["attention", attention],
      ] as const) {
        for (const profile of PROFILE_IDS) {
          calls.length = 0;
          const view = render(profile);
          await settled();
          await screen.findByText("Fix the disk").catch(() => undefined);
          const made = [
            ...new Set(calls.map((call) => `${call.route} ${JSON.stringify(call.init.params)}`)),
          ].sort();
          const key = `${name}`;
          const first = requests.get(key);
          if (first === undefined) {
            requests.set(key, made);
          } else {
            expect(made, `${name} in ${profile}`).toEqual(first);
          }
          view.unmount();
        }
      }
    },
    SLOW,
  );

  it.each(PROFILE_IDS)(
    "approving_from_the_control_room_in_%s_still_needs_the_typed_confirmation_and_sends_the_shown_plan_hash",
    async (profile) => {
      const base = waitingDetail("op-0", "Goal op-0");
      const risky = execution({
        ...(base.executions[0] as Execution),
        risk: "destructive",
        confirm_required: true,
      });
      route("GET /api/v1/operations/{id}", () => ({ ...base, executions: [risky] }));
      route(APPROVE, () => ({}));
      controlRoom(profile);
      const user = userEvent.setup();
      await user.click(await screen.findByRole("button", { name: "Approve" }));
      const dialog = within(await screen.findByRole("dialog"));
      const confirm = dialog.getByRole("button", { name: "Approve" });
      expect(confirm).toHaveAttribute("aria-disabled", "true");
      await user.type(dialog.getByRole("textbox"), "restart_container");
      expect(calls.filter((call) => call.route === APPROVE)).toHaveLength(0);
      await user.click(confirm);
      await waitFor(() => expect(calls.filter((call) => call.route === APPROVE)).toHaveLength(1));
      expect(calls.find((call) => call.route === APPROVE)?.init).toEqual({
        params: { path: { id: "op-0", step: risky.step } },
        body: { plan_hash: PLAN_HASH, confirm_text: "restart_container" },
      });
    },
    SLOW,
  );
});
