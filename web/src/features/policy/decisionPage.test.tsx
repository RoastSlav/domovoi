import { screen, waitFor } from "@testing-library/react";
import type { ShadowDetail } from "../../api/types";
import { failure, resetApi, route } from "../../test/apiMock";
import {
  DECISION_ERROR,
  DECISION_EXCEPTION,
  DECISION_LIMITS,
  DECISION_LOCKED,
  DECISION_UNAVAILABLE,
  DECISION_UNEVALUABLE,
  OVERVIEW_ACTIVE,
  OVERVIEW_BERLIN,
} from "../../test/policyFixtures";
import { renderAt } from "../../test/render";
import { DecisionPage } from "./DecisionPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

const DECISION = "GET /api/v1/policy/shadow/{execution_id}";

function show(detail: ShadowDetail, overview = OVERVIEW_ACTIVE) {
  route(DECISION, () => detail);
  route("GET /api/v1/policy", () => overview);
  route("GET /api/v1/hosts", () => []);
  renderAt(
    `/policy/decisions/${detail.execution_id}`,
    "/policy/decisions/:execution",
    <DecisionPage />,
  );
}

beforeEach(resetApi);

test("decision_page_explains_a_locked_deny_with_what_the_rules_alone_would_have_said", async () => {
  show(DECISION_LOCKED);
  expect(await screen.findByText("Locked by no-destroy")).toBeInTheDocument();
  expect(screen.getAllByText("Locked Deny").length).toBeGreaterThan(0);
  expect(screen.getByText(/Without the lock, the rules would have decided/)).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /approve/i })).not.toBeInTheDocument();
});

test("decision_page_marks_the_dangerous_exception_and_the_constraints", async () => {
  show(DECISION_EXCEPTION, OVERVIEW_BERLIN);
  expect(await screen.findByText("Allow overrides a broader Deny")).toBeInTheDocument();
  expect(
    screen.getByText("Rule plex-restart allows this and overrides the broader Deny no-destroy."),
  ).toBeInTheDocument();
  expect(screen.getByText("At most 3 per hour")).toBeInTheDocument();
  expect(screen.getAllByText("Lasts at most 10 min").length).toBeGreaterThan(0);
  expect(screen.getByText("Only for targets: plex")).toBeInTheDocument();
  await waitFor(() =>
    expect(screen.getByText(/Only between 22:00 and 06:00 Europe\/Berlin/)).toBeInTheDocument(),
  );
  expect(screen.getByText("An automatic Allow carries these limits.")).toBeInTheDocument();
});

test("decision_page_shows_hard_limits_apart_from_rules_and_the_missing_inputs", async () => {
  show(DECISION_LIMITS);
  expect(await screen.findByText("Hard limits")).toBeInTheDocument();
  expect(screen.getByText(/Rules alone:/)).toBeInTheDocument();
  expect(screen.getByText("Decided with missing inputs")).toBeInTheDocument();
  expect(screen.getByText("Approving this would need a typed confirmation.")).toBeInTheDocument();
});

test("decision_page_says_cannot_decide_for_an_unavailable_outcome", async () => {
  show(DECISION_UNAVAILABLE);
  expect(
    await screen.findByText("Cannot decide. Nothing runs and nothing is offered."),
  ).toBeInTheDocument();
});

test("a_deny_decision_is_never_offered_for_approval", async () => {
  show(DECISION_LOCKED);
  expect(
    await screen.findByText("Not offered for approval. To allow it, change the policy."),
  ).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /approve|deny/i })).not.toBeInTheDocument();
});

test("an_unevaluable_step_says_why_and_shows_no_explanation", async () => {
  show(DECISION_UNEVALUABLE);
  expect(
    await screen.findByText("This step was not evaluated, so there is no decision to explain."),
  ).toBeInTheDocument();
  expect(screen.queryByRole("heading", { name: "Active policy" })).not.toBeInTheDocument();
  expect(screen.queryByRole("heading", { name: "Shadow rules" })).not.toBeInTheDocument();
});

test("an_evaluation_error_is_one_warning_and_the_other_side_still_renders", async () => {
  show(DECISION_ERROR);
  expect(await screen.findByRole("heading", { name: "Shadow rules" })).toBeInTheDocument();
  expect(screen.getAllByRole("status").some((el) => el.dataset.tone === "warning")).toBe(true);
  expect(screen.getByText("No shadow rule matched.")).toBeInTheDocument();
});

test("an_unrecorded_step_is_an_empty_state_worded_by_the_recording_state", async () => {
  route(DECISION, () => failure(404, "not_found", "No such decision."));
  route("GET /api/v1/policy", () => OVERVIEW_ACTIVE);
  route("GET /api/v1/hosts", () => []);
  renderAt("/policy/decisions/x", "/policy/decisions/:execution", <DecisionPage />);
  expect(await screen.findByText("No decision recorded for this step")).toBeInTheDocument();
  expect(await screen.findByText(/recorded within a few seconds/)).toBeInTheDocument();
});

test("an_unrecorded_step_says_how_to_turn_recording_on_when_it_is_off", async () => {
  route(DECISION, () => failure(404, "not_found", "No such decision."));
  route("GET /api/v1/policy", () => ({ ...OVERVIEW_ACTIVE, shadow_enabled: false }));
  route("GET /api/v1/hosts", () => []);
  renderAt("/policy/decisions/x", "/policy/decisions/:execution", <DecisionPage />);
  expect(await screen.findByText(/Set shadow to true under \[policy\]/)).toBeInTheDocument();
});

test("every_recorded_decision_is_labelled_not_enforced", async () => {
  show(DECISION_EXCEPTION);
  await screen.findByText("Allow overrides a broader Deny");
  expect(screen.getAllByText("Not enforced").length).toBeGreaterThanOrEqual(3);
});
