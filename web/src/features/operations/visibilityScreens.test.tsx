import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Action, Execution, Host, OperationDetail, OperationSummary } from "../../api/types";
import type { TraceStep } from "../../api/operationTrace";
import { stepOutcomeMeta, riskMeta } from "../../design/tone";
import { PROFILE_IDS, type ProfileId } from "../../design/visibility";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { nas } from "../../test/fixtures";
import {
  execution,
  operationDetail,
  PLAN_HASH,
  TRACE_FIXTURES,
  type TraceFixtureName,
} from "../../test/operationFixtures";
import { showOperation, stepDetail } from "../../test/operationPage";
import { expectP0Everywhere, renderWithProfile } from "../../test/visibility";
import { IncidentOperations } from "./IncidentOperations";
import { OperationsPage } from "./OperationsPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

beforeEach(resetApi);

const STEP_ROUTE = "GET /api/v1/operations/{id}/steps/{step}";
const APPROVE = "POST /api/v1/operations/{id}/steps/{step}/approve";
const FIXTURES = Object.keys(TRACE_FIXTURES) as TraceFixtureName[];
const SLOW = 40_000;

// ---------------------------------------------------------------------------------------------
// An independent model of what is technical and what is P0, written from the product rules and
// deliberately not sharing code with the module under test.
// ---------------------------------------------------------------------------------------------

const NEVER_A_PROBLEM = new Set(["succeeded", "running", "pending", "skipped", "cancelled"]);
const ALWAYS_P0_KINDS = new Set(["approval", "execution", "verification", "question"]);
const TECHNICAL_KINDS = new Set(["evidence_request", "reasoning_turn"]);
const TERMINAL = new Set(["succeeded", "failed", "escalated", "cancelled"]);

const isP0 = (step: TraceStep) =>
  ALWAYS_P0_KINDS.has(step.kind) || !NEVER_A_PROBLEM.has(step.outcome);
const isTechnical = (step: TraceStep) =>
  TECHNICAL_KINDS.has(step.kind) && NEVER_A_PROBLEM.has(step.outcome);
const hasProblem = (step: TraceStep) => !NEVER_A_PROBLEM.has(step.outcome);

/** What the top-level list of one cycle should read: rows and, when folding, run toggles. */
function expectedOutline(steps: readonly TraceStep[], folding: boolean): string[] {
  const out: string[] = [];
  let run = 0;
  const flush = () => {
    if (run > 0) {
      out.push(`group:${run} technical ${run === 1 ? "step" : "steps"}`);
      run = 0;
    }
  };
  for (const step of steps) {
    if (folding && isTechnical(step)) {
      run += 1;
    } else {
      flush();
      out.push(`row:${step.id}`);
    }
  }
  flush();
  return out;
}

// ---------------------------------------------------------------------------------------------
// Fixtures and page helpers
// ---------------------------------------------------------------------------------------------

type Spec = [kind: string, outcome: string, parent?: number | null];

const sid = (n: number) => `s${String(n).padStart(2, "0")}`;

/** A finished operation with exactly these steps (parent is a 1-based step number). */
function withSteps(specs: Spec[]): OperationDetail {
  const base = operationDetail("verified-success");
  const template = base.trace.steps[0] as TraceStep;
  const steps = specs.map(([kind, outcome, parent], index) => ({
    ...template,
    id: sid(index + 1),
    kind,
    outcome,
    parent: parent ? sid(parent) : null,
    cycle: 0,
    summary: null,
    started_at: null,
    ended_at: null,
  }));
  return { ...base, trace: { ...base.trace, steps, edges: [] }, executions: [] } as OperationDetail;
}

function mutateStep(
  detail: OperationDetail,
  index: number,
  change: Partial<TraceStep>,
): OperationDetail {
  const steps = detail.trace.steps.map((step, at) =>
    at === index ? { ...step, ...change } : step,
  );
  return { ...detail, trace: { ...detail.trace, steps } } as OperationDetail;
}

interface Opts {
  pages?: Record<string, ProfileId>;
  profile?: ProfileId;
  reason?: string | null;
  search?: string;
}

/** Renders the operation page and waits until its data is on screen. */
async function open(detail: OperationDetail, opts: Opts = {}) {
  const view = showOperation(detail, {
    search: opts.search ?? "",
    profile: opts.profile ?? "detailed",
    pages: opts.pages ?? {},
  });
  if (opts.reason !== undefined) {
    const reason = opts.reason;
    route(STEP_ROUTE, (init) => ({
      ...stepDetail(detail, init.params?.path?.step ?? ""),
      reason,
    }));
  }
  await screen.findByRole("heading", { level: 1, name: detail.operation.goal });
  return view;
}

const stepQuery = (id: string) => `?step=${id}`;
const rows = () => [
  ...document.querySelectorAll<HTMLElement>('[data-visibility-id="operation.step"]'),
];
const rowId = (row: Element) => row.querySelector("a")?.getAttribute("href")?.split("?step=")[1];
const rowIds = () => rows().map(rowId);
const blockOf = (id: string, root: ParentNode = document) =>
  root.querySelector<HTMLElement>(`[data-visibility-id="${id}"]`);
const toggleLabels = (root: ParentNode = document) =>
  [...root.querySelectorAll("button")]
    .map((button) => /^(\d+ technical steps?)/.exec(button.textContent ?? "")?.[1])
    .filter((label): label is string => label !== undefined);

/** The top-level list of the single-cycle timeline, as rows and run toggles in order. */
function outline(): string[] {
  const list = document.querySelector('ol[aria-label="Steps"]');
  return [...(list?.children ?? [])].map((item) => {
    const id = item.getAttribute("data-visibility-id") === "operation.step" && rowId(item);
    return id
      ? `row:${id}`
      : `group:${/^(\d+ technical steps?)/.exec(item.textContent ?? "")?.[1]}`;
  });
}

const inspector = () => screen.findByRole("complementary", { name: "Step inspector" });
const pageText = (id: string) => blockOf(id)?.textContent ?? "";

async function settle(client: { isFetching: () => number }) {
  await waitFor(() => expect(client.isFetching()).toBe(0));
}

// ---------------------------------------------------------------------------------------------
// Step outline
// ---------------------------------------------------------------------------------------------

describe("step outline", () => {
  it("standard_folds_consecutive_technical_steps_into_one_group_each_and_keeps_every_other_row", async () => {
    const waiting = operationDetail("waiting-approval");
    for (const profile of ["standard", "essential"] as const) {
      const view = await open(waiting, { profile });
      expect(outline()).toEqual(expectedOutline(waiting.trace.steps, true));
      expect(toggleLabels()).toEqual(["2 technical steps"]);
      expect(rows()).toHaveLength(waiting.trace.steps.length - 2);
      view.unmount();
    }

    // Several runs of different lengths, a run of one, and a P1 step between runs.
    const custom = withSteps([
      ["evidence_request", "succeeded"],
      ["reasoning_turn", "succeeded"],
      ["plan", "succeeded"],
      ["evidence_request", "running"],
      ["note", "succeeded"],
      ["reasoning_turn", "succeeded"],
      ["reasoning_turn", "pending"],
      ["evidence_request", "skipped"],
      ["approval", "succeeded"],
    ]);
    for (const profile of ["standard", "essential"] as const) {
      const view = await open(custom, { profile });
      expect(outline()).toEqual([
        "group:2 technical steps",
        `row:${sid(3)}`,
        "group:1 technical step",
        `row:${sid(5)}`,
        "group:3 technical steps",
        `row:${sid(9)}`,
      ]);
      expect(outline()).toEqual(expectedOutline(custom.trace.steps, true));
      view.unmount();
    }
  });

  it("detailed_shows_every_step_row_and_no_group", async () => {
    const waiting = operationDetail("waiting-approval");
    await open(waiting, { profile: "detailed" });
    expect(toggleLabels()).toEqual([]);
    expect(outline()).toEqual(expectedOutline(waiting.trace.steps, false));
  });

  it("opening_a_group_shows_its_rows_in_order_with_their_outcomes", async () => {
    const waiting = operationDetail("waiting-approval");
    await open(waiting, { profile: "standard" });
    const user = userEvent.setup();
    const toggle = screen.getByRole("button", { name: /^2 technical steps/ });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    const group = screen.getByRole("group", { name: "2 technical steps" });
    expect(
      within(group)
        .getAllByRole("link")
        .map((link) => link.getAttribute("href")?.split("?step=")[1]),
    ).toEqual(waiting.trace.steps.slice(0, 2).map((step) => step.id));
    // With the only run open, every step is on screen again, in trace order, each with its outcome.
    expect(rowIds()).toEqual(waiting.trace.steps.map((step) => step.id));
    rows().forEach((row, index) => {
      const step = waiting.trace.steps[index] as TraceStep;
      expect(row).toHaveTextContent(stepOutcomeMeta(step.outcome).label);
    });
    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(rows()).toHaveLength(waiting.trace.steps.length - 2);
  });

  it("a_technical_step_with_a_problem_outcome_is_not_folded_in_any_profile", async () => {
    const waiting = operationDetail("waiting-approval");
    const evidenceAt = waiting.trace.steps.findIndex((step) => step.kind === "evidence_request");
    const broken = mutateStep(waiting, evidenceAt, { outcome: "refused" });
    const evidenceId = (broken.trace.steps[evidenceAt] as TraceStep).id;
    for (const profile of PROFILE_IDS) {
      const view = await open(broken, { profile });
      expect(rowIds(), `${profile}: the refused step stays a row`).toContain(evidenceId);
      const row = rows().find((candidate) => rowId(candidate) === evidenceId) as HTMLElement;
      expect(row).toHaveAttribute("data-visibility", "P0");
      expect(row).toHaveTextContent(stepOutcomeMeta("refused").label);
      expect(outline()).toEqual(expectedOutline(broken.trace.steps, profile !== "detailed"));
      // Only the reasoning turn before it is left to fold.
      expect(toggleLabels()).toEqual(profile === "detailed" ? [] : ["1 technical step"]);
      view.unmount();
    }
  });

  it("every_problem_outcome_keeps_a_technical_step_in_view_in_standard", async () => {
    const outcomes = ["failed", "refused", "not_run", "unavailable", "outcome_unknown"];
    const detail = withSteps(
      outcomes.flatMap((outcome): Spec[] => [
        ["evidence_request", outcome],
        ["reasoning_turn", "succeeded"],
      ]),
    );
    await open(detail, { profile: "standard" });
    const visible = new Set(rowIds());
    outcomes.forEach((_, index) => {
      expect(visible.has(sid(index * 2 + 1))).toBe(true);
      expect(visible.has(sid(index * 2 + 2))).toBe(false);
    });
  });

  it("a_step_link_to_a_technical_step_opens_its_group_and_the_inspector", async () => {
    const waiting = operationDetail("waiting-approval");
    const technical = waiting.trace.steps[0] as TraceStep;
    expect(isTechnical(technical)).toBe(true);
    for (const profile of ["essential", "standard"] as const) {
      const view = await open(waiting, { profile, search: stepQuery(technical.id) });
      const toggle = screen.getByRole("button", { name: /^2 technical steps/ });
      expect(toggle).toHaveAttribute("aria-expanded", "true");
      expect(rowIds()).toContain(technical.id);
      const row = rows().find((candidate) => rowId(candidate) === technical.id) as HTMLElement;
      expect(within(row).getByRole("link")).toHaveAttribute("aria-current", "true");
      const panel = await inspector();
      expect(blockOf("inspector.outcome", panel)).toHaveTextContent(
        stepOutcomeMeta(technical.outcome).label,
      );
      view.unmount();
    }
  });

  it("tree_view_does_not_fold_a_technical_parent_that_has_a_p0_child", async () => {
    const detail = withSteps([
      ["reasoning_turn", "succeeded"], // s01, root, technical on its own
      ["evidence_request", "succeeded", 1], // s02, technical child of s01
      ["verification", "succeeded", 1], // s03, P0 child of s01
      ["evidence_request", "succeeded"], // s04, technical root
      ["reasoning_turn", "succeeded"], // s05, technical root
    ]);
    await open(detail, { profile: "standard" });
    // The timeline ignores parents: both technical runs fold.
    expect(rowIds()).toEqual([sid(3)]);
    expect(toggleLabels()).toEqual(["2 technical steps", "2 technical steps"]);

    await userEvent.setup().click(screen.getByRole("radio", { name: "Tree" }));
    expect(rowIds()).toEqual([sid(1), sid(3)]);
    const parent = rows().find((row) => rowId(row) === sid(1)) as HTMLElement;
    expect(parent).toHaveAttribute("data-visibility", "P0");
    expect(toggleLabels().sort()).toEqual(["1 technical step", "2 technical steps"]);
    // The folded child under it is the one technical step; the P0 child is a row.
    expect(parent.nextElementSibling).toHaveTextContent("1 technical step");
  });

  it("a_technical_parent_without_a_p0_descendant_folds_in_tree_view", async () => {
    const detail = withSteps([
      ["reasoning_turn", "succeeded"],
      ["evidence_request", "succeeded", 1],
      ["note", "succeeded"],
    ]);
    await open(detail, { profile: "standard" });
    await userEvent.setup().click(screen.getByRole("radio", { name: "Tree" }));
    expect(rowIds()).toEqual([sid(3)]);
    expect(toggleLabels()).toEqual(["1 technical step"]);
  });
});

// ---------------------------------------------------------------------------------------------
// Operation details
// ---------------------------------------------------------------------------------------------

describe("operation details", () => {
  it("essential_hides_operation_metadata_behind_plus_3_details_and_the_button_reveals_it", async () => {
    await open(operationDetail("verified-success"), { profile: "essential" });
    const user = userEvent.setup();
    expect(screen.queryByText("Kind")).toBeNull();
    expect(screen.queryByText("Started")).toBeNull();
    expect(screen.queryByText("Ended")).toBeNull();
    const more = screen.getByRole("button", { name: "+3 details" });
    expect(more).toHaveAttribute("aria-expanded", "false");
    await user.click(more);
    expect(screen.getByText("Kind")).toBeVisible();
    expect(screen.getByText("Started")).toBeVisible();
    expect(screen.getByText("Ended")).toBeVisible();
    const less = screen.getByRole("button", { name: "Hide 3 details" });
    expect(less).toHaveAttribute("aria-expanded", "true");
    await user.click(less);
    expect(screen.queryByText("Kind")).toBeNull();
    expect(screen.getByRole("button", { name: "+3 details" })).toBeVisible();
  });

  it("standard_collapses_operation_metadata_into_a_toggle_with_the_count", async () => {
    await open(operationDetail("verified-success"), { profile: "standard" });
    const user = userEvent.setup();
    expect(screen.queryByText("Kind")).toBeNull();
    expect(screen.queryByRole("button", { name: /details$/ })).toBeNull();
    const toggle = screen.getByRole("button", { name: "Kind and times 3 items" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    await user.click(toggle);
    expect(screen.getByText("Kind")).toBeVisible();
    expect(screen.getByText("Started")).toBeVisible();
    expect(screen.getByText("Ended")).toBeVisible();
  });

  it("detailed_shows_operation_metadata_in_place_with_no_toggle", async () => {
    await open(operationDetail("verified-success"), { profile: "detailed" });
    expect(screen.getByText("Kind")).toBeVisible();
    expect(screen.queryByRole("button", { name: /Kind and times|details/ })).toBeNull();
  });

  it("pinned_subjects_budgets_and_incidents_stay_shown_in_every_profile", async () => {
    for (const name of ["waiting-approval", "question-and-budget-pause"] as const) {
      const detail = operationDetail(name);
      const subject = detail.trace.operation.pinned[0];
      for (const profile of PROFILE_IDS) {
        const view = await open(detail, { profile });
        const scope = blockOf("operation.scope") as HTMLElement;
        expect(scope).toHaveAttribute("data-presentation", "shown");
        expect(scope).toHaveTextContent("Pinned subjects");
        expect(scope).toHaveTextContent(`${subject?.check} on ${subject?.target}`);
        for (const id of ["operation.budgets", "operation.incidents"]) {
          expect(blockOf(id), `${id} in ${profile}`).toHaveAttribute("data-presentation", "shown");
        }
        expect(blockOf("operation.budgets")).toHaveTextContent("Budgets");
        const incident = detail.operation.incidents[0] as string;
        expect(
          within(blockOf("operation.incidents") as HTMLElement).getByRole("link", {
            name: incident,
          }),
        ).toHaveAttribute("href", `/incidents/${incident}`);
        view.unmount();
      }
    }
  });

  it(
    "banner_and_waiting_for_you_are_identical_in_every_profile",
    async () => {
      let withBanner = 0;
      let withWaiting = 0;
      for (const name of FIXTURES) {
        const detail = operationDetail(name);
        const seen: { banner: string; waiting: string; profile: ProfileId }[] = [];
        for (const profile of PROFILE_IDS) {
          const view = await open(detail, { profile });
          seen.push({
            profile,
            banner: pageText("operation.banner"),
            waiting: pageText("operation.waiting"),
          });
          view.unmount();
        }
        // A live countdown ticks between renders; its seconds are not what this compares.
        const steady = (text: string | undefined) =>
          text?.replace(/Expires in [\d:]+/g, "Expires in");
        for (const entry of seen) {
          expect(steady(entry.banner), `${name} banner in ${entry.profile}`).toBe(
            steady(seen[0]?.banner),
          );
          expect(steady(entry.waiting), `${name} waiting in ${entry.profile}`).toBe(
            steady(seen[0]?.waiting),
          );
        }
        withBanner += seen[0]?.banner ? 1 : 0;
        withWaiting += seen[0]?.waiting ? 1 : 0;
      }
      expect(withBanner, "some fixture has a banner with content").toBeGreaterThan(0);
      expect(withWaiting, "some fixture has something waiting").toBeGreaterThan(0);
    },
    SLOW,
  );
});

// ---------------------------------------------------------------------------------------------
// Step inspector
// ---------------------------------------------------------------------------------------------

describe("step inspector", () => {
  const verified = operationDetail("verified-success");
  const executionStep = verified.trace.steps.find((step) => step.kind === "execution") as TraceStep;
  const edgeCount = verified.trace.edges.filter(
    (edge) => edge.from === executionStep.id || edge.to === executionStep.id,
  ).length;

  it("inspector_in_standard_collapses_connections_intent_and_output_and_keeps_outcome_and_summary", async () => {
    await open(verified, { profile: "standard", search: stepQuery(executionStep.id) });
    const panel = await inspector();
    const user = userEvent.setup();
    expect(edgeCount).toBeGreaterThan(0);
    expect(blockOf("inspector.outcome", panel)).toHaveTextContent(
      stepOutcomeMeta(executionStep.outcome).label,
    );
    expect(blockOf("inspector.outcome", panel)).toHaveAttribute("data-presentation", "shown");
    expect(blockOf("inspector.summary", panel)).toHaveTextContent(executionStep.summary as string);
    expect(blockOf("inspector.summary", panel)).toHaveAttribute("data-presentation", "shown");
    expect(blockOf("inspector.kind", panel)).toHaveAttribute("data-presentation", "shown");

    const connections = blockOf("inspector.connections", panel) as HTMLElement;
    expect(connections).toHaveAttribute("data-presentation", "collapsed");
    expect(within(connections).queryAllByRole("link")).toHaveLength(0);
    await user.click(
      within(connections).getByRole("button", { name: `Connections ${edgeCount} items` }),
    );
    expect(within(connections).getAllByRole("link")).toHaveLength(edgeCount);

    const intent = (await waitFor(() => blockOf("inspector.intent", panel))) as HTMLElement;
    expect(intent).toHaveAttribute("data-presentation", "collapsed");
    expect(within(intent).queryByText("Which container?")).toBeNull();
    await user.click(within(intent).getByRole("button", { name: "Intent 1 items" }));
    expect(within(intent).getByText("Which container?")).toBeVisible();

    const output = blockOf("inspector.output", panel) as HTMLElement;
    expect(output).toHaveAttribute("data-presentation", "collapsed");
    expect(within(output).queryByText("plex is down")).toBeNull();
    await user.click(within(output).getByRole("button", { name: "Step output" }));
    expect(within(output).getByText("plex is down")).toBeVisible();

    const times = blockOf("inspector.times", panel) as HTMLElement;
    expect(times).toHaveAttribute("data-presentation", "collapsed");
    expect(within(times).getByRole("button", { name: "Times and phase 2 items" })).toBeVisible();
  });

  it("inspector_in_detailed_shows_connections_intent_output_and_times_in_place", async () => {
    await open(verified, { profile: "detailed", search: stepQuery(executionStep.id) });
    const panel = await inspector();
    expect(await within(panel).findByText("plex is down")).toBeVisible();
    expect(within(panel).getByText("Which container?")).toBeVisible();
    expect(within(panel).getAllByRole("link")).toHaveLength(edgeCount);
    expect(within(panel).getByText(/^Phase /)).toBeVisible();
    expect(within(panel).queryByRole("button", { name: /items$|details$/ })).toBeNull();
  });

  it("inspector_reason_of_a_failed_step_is_shown_in_essential", async () => {
    const detail = operationDetail("question-and-budget-pause");
    const refused = detail.trace.steps.find((step) => step.outcome === "refused") as TraceStep;
    expect(hasProblem(refused)).toBe(true);
    await open(detail, {
      profile: "essential",
      search: stepQuery(refused.id),
      reason: "the host refused to run it",
    });
    const panel = await inspector();
    const reason = await waitFor(() => {
      const found = blockOf("inspector.reason", panel);
      expect(found).not.toBeNull();
      return found as HTMLElement;
    });
    expect(reason).toHaveAttribute("data-visibility", "P0");
    expect(reason).toHaveAttribute("data-presentation", "shown");
    expect(reason).toHaveTextContent("the host refused to run it");
  });

  it("inspector_shows_no_reason_for_a_step_that_did_not_go_wrong", async () => {
    const detail = operationDetail("question-and-budget-pause");
    const fine = detail.trace.steps.find((step) => step.outcome === "succeeded") as TraceStep;
    const { client } = await open(detail, {
      profile: "essential",
      search: stepQuery(fine.id),
      reason: "a reason that must not show",
    });
    await inspector();
    await settle(client);
    expect(blockOf("inspector.reason")).toBeNull();
    expect(screen.queryByText("a reason that must not show")).toBeNull();
  });

  it("inspector_times_and_phase_are_behind_plus_details_in_essential", async () => {
    await open(verified, { profile: "essential", search: stepQuery(executionStep.id) });
    const panel = await inspector();
    const user = userEvent.setup();
    expect(within(panel).queryByText(/^Phase /)).toBeNull();
    expect(blockOf("inspector.outcome", panel)).toHaveAttribute("data-presentation", "shown");
    expect(blockOf("inspector.summary", panel)).toHaveAttribute("data-presentation", "shown");
    await user.click(within(panel).getByRole("button", { name: "+2 details" }));
    expect(within(panel).getByText(/^Phase /)).toBeVisible();
    expect(within(panel).getByText(/Started/)).toBeVisible();
    await user.click(within(panel).getByRole("button", { name: "Hide 2 details" }));
    expect(within(panel).queryByText(/^Phase /)).toBeNull();
  });
});

// ---------------------------------------------------------------------------------------------
// Approval card
// ---------------------------------------------------------------------------------------------

describe("approval card", () => {
  const waiting = operationDetail("waiting-approval");

  it("approval_card_is_complete_in_every_profile", async () => {
    for (const profile of PROFILE_IDS) {
      const view = await open(waiting, { profile });
      const card = screen.getByRole("group", { name: "Action restart_container" });
      expect(within(card).getByText("on nas")).toBeVisible();
      expect(within(card).getByText("Check container.down")).toBeVisible();
      expect(within(card).getByText("Target plex")).toBeVisible();
      expect(within(card).getByText("name = plex")).toBeVisible();
      expect(within(card).getByText(riskMeta("write").label)).toBeVisible();
      expect(within(card).getByText(/Expires in/)).toBeVisible();
      expect(within(card).getByText(`Plan ${PLAN_HASH.slice(0, 8)}`)).toBeVisible();
      expect(within(card).getByText("v2")).toBeVisible();
      expect(within(card).getByRole("button", { name: "Approve" })).toBeVisible();
      expect(within(card).getByRole("button", { name: "Deny" })).toBeVisible();
      expect(card.closest('[data-visibility="P0"]')).not.toBeNull();
      view.unmount();
    }
  });

  it.each(PROFILE_IDS)(
    "approving_in_%s_still_asks_for_the_typed_confirmation_and_sends_the_shown_plan_hash",
    async (profile) => {
      const base = operationDetail("waiting-approval");
      const risky = execution({
        ...(base.executions[0] as Execution),
        risk: "destructive",
        confirm_required: true,
      });
      const detail = { ...base, executions: [risky] };
      await open(detail, { profile });
      route(APPROVE, () => ({}));
      const user = userEvent.setup();
      await user.click(screen.getByRole("button", { name: "Approve" }));
      const dialog = within(await screen.findByRole("dialog"));
      const confirm = dialog.getByRole("button", { name: "Approve" });
      expect(confirm).toHaveAttribute("aria-disabled", "true");
      await user.type(dialog.getByRole("textbox"), "restart_container");
      expect(confirm).not.toHaveAttribute("aria-disabled", "true");
      expect(calls.filter((call) => call.route === APPROVE)).toHaveLength(0);
      await user.click(confirm);
      await waitFor(() => expect(calls.filter((call) => call.route === APPROVE)).toHaveLength(1));
      expect(calls.find((call) => call.route === APPROVE)?.init).toEqual({
        params: { path: { id: detail.operation.id, step: risky.step } },
        body: { plan_hash: PLAN_HASH, confirm_text: "restart_container" },
      });
    },
  );

  it("deny_is_available_and_sends_nothing_to_approve_in_essential", async () => {
    await open(waiting, { profile: "essential" });
    route("POST /api/v1/operations/{id}/steps/{step}/deny", () => ({}));
    await userEvent.setup().click(screen.getByRole("button", { name: "Deny" }));
    expect(await screen.findByText("Denied restart_container. Nothing ran.")).toBeVisible();
    expect(calls.filter((call) => call.route === APPROVE)).toHaveLength(0);
  });
});

// ---------------------------------------------------------------------------------------------
// Per-page override and requests
// ---------------------------------------------------------------------------------------------

describe("page override", () => {
  it("a_page_override_beats_the_default_for_that_page_only", async () => {
    const waiting = operationDetail("waiting-approval");

    // Default detailed, this page essential: this page folds.
    let view = await open(waiting, { profile: "detailed", pages: { operation: "essential" } });
    expect(toggleLabels()).toEqual(["2 technical steps"]);
    expect(screen.getByRole("button", { name: "+3 details" })).toBeVisible();
    view.unmount();

    // Default essential, this page detailed: nothing folds.
    view = await open(waiting, { profile: "essential", pages: { operation: "detailed" } });
    expect(toggleLabels()).toEqual([]);
    expect(rows()).toHaveLength(waiting.trace.steps.length);
    expect(screen.getByText("Kind")).toBeVisible();
    view.unmount();

    // An override for another page leaves this page at the default.
    view = await open(waiting, { profile: "detailed", pages: { operations: "essential" } });
    expect(toggleLabels()).toEqual([]);
    expect(rows()).toHaveLength(waiting.trace.steps.length);
    view.unmount();
    view = await open(waiting, { profile: "essential", pages: { operations: "detailed" } });
    expect(toggleLabels()).toEqual(["2 technical steps"]);
    view.unmount();
  });

  it("the_profile_never_changes_which_requests_are_made", async () => {
    const detail = operationDetail("verified-success");
    const step = detail.trace.steps.find(
      (candidate) => candidate.kind === "execution",
    ) as TraceStep;
    const made: Record<string, string[]> = {};
    for (const profile of ["detailed", "essential"] as const) {
      resetApi();
      const { client, unmount } = await open(detail, { profile, search: stepQuery(step.id) });
      await inspector();
      await waitFor(() => expect(calls.some((call) => call.route === STEP_ROUTE)).toBe(true));
      await settle(client);
      made[profile] = calls
        // Similar past operations are technical detail: Essential asks for them only once opened.
        .filter((call) => !call.route.includes("preferences") && !call.route.endsWith("/similar"))
        .map((call) => `${call.route} ${JSON.stringify(call.init)}`)
        .sort();
      unmount();
    }
    expect(made["detailed"]?.length).toBeGreaterThan(2);
    expect(made["essential"]).toEqual(made["detailed"]);
  });
});

// ---------------------------------------------------------------------------------------------
// Operations list and the incident page
// ---------------------------------------------------------------------------------------------

function summary(id: string, status: string, over: Partial<OperationSummary> = {}) {
  return {
    id,
    kind: "investigate",
    goal: `Goal ${id}`,
    status,
    waiting_for: [],
    cycle: 0,
    incidents: [],
    hosts: [nas.id],
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-02T00:00:00Z",
    ...over,
  } as OperationSummary;
}

const legacyAction = {
  id: "a1",
  skill: "restart_container",
  host_id: nas.id,
  incident_id: "inc-9",
  status: "proposed",
  legacy: true,
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
} as unknown as Action;

function serveList(items: OperationSummary[], actions: Action[] = []) {
  route("GET /api/v1/hosts", () => [nas as Host]);
  route("GET /api/v1/operations", () => ({ items, next_cursor: null, as_of_seq: 1 }));
  route("GET /api/v1/actions", () => ({ items: actions, next_cursor: null, as_of_seq: 1 }));
}

const LIST_OPERATIONS = [
  summary("o1", "waiting_approval", { waiting_for: ["approval"] }),
  summary("o2", "running"),
  summary("o3", "paused"),
  summary("o4", "failed"),
];

describe("operations list", () => {
  it(
    "the_operations_list_is_the_same_in_every_profile",
    async () => {
      const scenarios: { name: string; wait: () => Promise<unknown>; serve: () => void }[] = [
        {
          name: "populated",
          serve: () => serveList(LIST_OPERATIONS, [legacyAction]),
          wait: () => screen.findByText("Goal o1"),
        },
        {
          name: "empty",
          serve: () => serveList([]),
          wait: () => screen.findByText("No operations yet"),
        },
        {
          name: "failed",
          serve: () => {
            serveList([]);
            route("GET /api/v1/operations", () => failure(500, "internal", "boom"));
          },
          wait: () => screen.findByRole("button", { name: "Retry" }),
        },
      ];
      for (const scenario of scenarios) {
        const seen: { profile: ProfileId; text: string; rows: number; ids: string[] }[] = [];
        for (const profile of PROFILE_IDS) {
          scenario.serve();
          const view = renderWithProfile(profile, "/operations", "/operations", <OperationsPage />);
          await scenario.wait();
          seen.push({
            profile,
            text: document.body.textContent ?? "",
            rows: screen.queryAllByRole("row").length,
            ids: [...document.querySelectorAll("[data-visibility-id]")].map(
              (element) =>
                `${element.getAttribute("data-visibility-id")}:${element.getAttribute("data-visibility")}`,
            ),
          });
          view.unmount();
        }
        for (const entry of seen) {
          expect(entry, `${scenario.name} in ${entry.profile}`).toEqual({
            ...seen[0],
            profile: entry.profile,
          });
        }
        if (scenario.name === "populated") {
          expect(seen[0]?.rows).toBe(LIST_OPERATIONS.length + 1 + 1);
          expect(seen[0]?.text).toContain("Legacy");
        }
      }
    },
    SLOW,
  );

  it("the_operations_list_ignores_the_operation_pages_override", async () => {
    serveList(LIST_OPERATIONS);
    renderWithProfile("detailed", "/operations", "/operations", <OperationsPage />, {
      operation: "essential",
    });
    await screen.findByText("Goal o1");
    expect(screen.getAllByRole("row")).toHaveLength(LIST_OPERATIONS.length + 1);
  });
});

describe("detailed has no folds", () => {
  it(
    "detailed_has_no_collapsed_blocks_on_operations_screens",
    async () => {
      for (const name of FIXTURES) {
        const detail = operationDetail(name);
        const first = detail.trace.steps.find((step) => step.kind === "execution") as TraceStep;
        const view = await open(detail, { profile: "detailed", search: stepQuery(first.id) });
        await inspector();
        await screen.findByText("Which container?");
        expect(document.querySelectorAll('[data-presentation="collapsed"]'), name).toHaveLength(0);
        expect(toggleLabels(), name).toEqual([]);
        expect(screen.queryByRole("button", { name: /^\+\d+ details?$/ }), name).toBeNull();
        expect(
          screen.queryByRole("button", {
            name: /^(Connections|Intent|Step output|Times and phase|Kind and times)/,
          }),
          name,
        ).toBeNull();
        expect(rows(), name).toHaveLength(detail.trace.steps.length);
        view.unmount();
      }

      serveList(LIST_OPERATIONS);
      renderWithProfile("detailed", "/operations", "/operations", <OperationsPage />);
      await screen.findByText("Goal o1");
      expect(document.querySelectorAll('[data-presentation="collapsed"]')).toHaveLength(0);
      expect(screen.queryByRole("button", { name: /details?$/ })).toBeNull();
    },
    SLOW,
  );
});

// ---------------------------------------------------------------------------------------------
// P0 everywhere
// ---------------------------------------------------------------------------------------------

/** Everything the page must show whatever the detail level, worked out from the data alone. */
function expectation(detail: OperationDetail) {
  const steps = detail.trace.steps;
  const inspected = steps.find((step) => step.kind === "execution") ?? (steps[0] as TraceStep);
  const answered = new Set(
    detail.trace.edges.filter((edge) => edge.kind === "answers").map((edge) => edge.to),
  );
  const awaiting = detail.executions.some((item) => item.state === "awaiting_approval");
  const asking = steps.some(
    (step) => step.kind === "question" && step.outcome === "pending" && !answered.has(step.id),
  );
  const status = detail.operation.status;
  const ids = ["operation.title", "operation.scope", "operation.banner", "inspector.outcome"];
  if (!TERMINAL.has(status)) {
    ids.push("operation.controls");
  }
  if (awaiting || asking) {
    ids.push("operation.waiting");
  }
  if (status === "running" || status === "queued") {
    ids.push("operation.progress");
  }
  if (awaiting || inspected.kind === "execution") {
    ids.push("approval.card");
  }
  if (inspected.kind === "execution") {
    ids.push("inspector.approval");
  }
  if (hasProblem(inspected)) {
    ids.push("inspector.reason");
  }
  return {
    ids,
    inspected,
    p0Rows: steps.filter(isP0).length,
    banner: [
      "paused",
      "waiting_approval",
      "waiting_user",
      "waiting_agent",
      "failed",
      "escalated",
    ].includes(status),
    waiting: awaiting || asking,
    problem: steps.some(hasProblem),
  };
}

const REASON = "the agent said why";

/** The operation page with its inspector fully loaded, for `expectP0Everywhere`. */
async function renderReady(
  detail: OperationDetail,
  profile: ProfileId,
  pages?: Record<string, ProfileId>,
) {
  const want = expectation(detail);
  const view = await open(detail, {
    profile,
    pages: pages ?? {},
    search: stepQuery(want.inspected.id),
    reason: REASON,
  });
  await inspector();
  if (hasProblem(want.inspected)) {
    await screen.findByText(REASON);
  }
  await settle(view.client);
  expect(
    document.querySelectorAll('[data-visibility-id="operation.step"][data-visibility="P0"]'),
    `${detail.operation.id}: P0 step rows in ${profile}`,
  ).toHaveLength(want.p0Rows);
  return view;
}

describe("P0 is visible in every profile", () => {
  it("fixtures_cover_a_banner_a_waiting_approval_and_a_problem_step", () => {
    const all = FIXTURES.map((name) => expectation(operationDetail(name)));
    expect(all.some((item) => item.banner)).toBe(true);
    expect(all.some((item) => item.waiting)).toBe(true);
    expect(all.some((item) => item.problem)).toBe(true);
    expect(all.some((item) => hasProblem(item.inspected))).toBe(true);
    expect(all.some((item) => item.ids.includes("operation.progress"))).toBe(true);
    expect(all.some((item) => !item.ids.includes("operation.controls"))).toBe(true);
  });

  it.each(FIXTURES)(
    "p0_visible_in_every_profile (%s)",
    async (name) => {
      const detail = operationDetail(name);
      const want = expectation(detail);
      await expectP0Everywhere({
        expectedIds: want.ids,
        render: (profile) => renderReady(detail, profile),
      });
      // The fixtures that expect a banner or a waiting block really draw one, with text in it.
      const view = await renderReady(detail, "essential");
      if (want.banner) {
        expect(pageText("operation.banner")).not.toBe("");
      }
      if (want.waiting) {
        expect(pageText("operation.waiting")).not.toBe("");
      }
      view.unmount();
    },
    SLOW,
  );

  it.each(["waiting-approval", "question-and-budget-pause"] as const)(
    "p0_visible_in_every_profile_with_a_page_override (%s)",
    async (name) => {
      const detail = operationDetail(name);
      const ids = expectation(detail).ids;
      // Default detailed, the page walks through every level.
      await expectP0Everywhere({
        expectedIds: ids,
        render: (level) => renderReady(detail, "detailed", { operation: level }),
      });
      // Default essential, the page walks through every level.
      await expectP0Everywhere({
        expectedIds: ids,
        render: (level) => renderReady(detail, "essential", { operation: level }),
      });
      // Every default under a fixed override.
      for (const override of ["essential", "detailed"] as const) {
        await expectP0Everywhere({
          expectedIds: ids,
          render: (profile) => renderReady(detail, profile, { operation: override }),
        });
      }
    },
    SLOW,
  );

  it(
    "p0_visible_in_every_profile_on_the_operations_list",
    async () => {
      await expectP0Everywhere({
        expectedIds: ["operations.table", "operations.state"],
        render: async (profile) => {
          serveList(LIST_OPERATIONS, [legacyAction]);
          const view = renderWithProfile(profile, "/operations", "/operations", <OperationsPage />);
          await screen.findByText("Goal o1");
          return view;
        },
      });
      await expectP0Everywhere({
        expectedIds: ["operations.state"],
        render: async (profile) => {
          serveList([]);
          const view = renderWithProfile(profile, "/operations", "/operations", <OperationsPage />);
          await screen.findByText("No operations yet");
          return view;
        },
      });
      await expectP0Everywhere({
        expectedIds: ["operations.state"],
        render: async (profile) => {
          serveList([]);
          route("GET /api/v1/operations", () => failure(500, "internal", "boom"));
          const view = renderWithProfile(profile, "/operations", "/operations", <OperationsPage />);
          await screen.findByRole("button", { name: "Retry" });
          return view;
        },
      });
    },
    SLOW,
  );

  it("p0_visible_in_every_profile_on_the_operations_list_with_a_page_override", async () => {
    await expectP0Everywhere({
      expectedIds: ["operations.table", "operations.state"],
      render: async (level) => {
        serveList(LIST_OPERATIONS);
        const view = renderWithProfile(
          "detailed",
          "/operations",
          "/operations",
          <OperationsPage />,
          {
            operations: level,
          },
        );
        await screen.findByText("Goal o1");
        return view;
      },
    });
  });

  it("p0_visible_in_every_profile_on_the_incident_investigations", async () => {
    const operations = [summary("o1", "running"), summary("o2", "failed"), summary("o3", "paused")];
    await expectP0Everywhere({
      expectedIds: ["incident.investigation-status"],
      render: async (profile) => {
        const view = renderWithProfile(
          profile,
          "/incidents/inc-1",
          "/incidents/:id",
          <IncidentOperations operations={operations} />,
        );
        expect(
          document.querySelectorAll('[data-visibility-id="incident.investigation-status"]'),
        ).toHaveLength(operations.length);
        await screen.findByRole("link", { name: "Goal o1" });
        return view;
      },
    });
  });
});
