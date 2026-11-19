import failedVerification from "../test/fixtures/operation-trace/failed-verification-new-cycle.json";
import outcomeUnknown from "../test/fixtures/operation-trace/outcome-unknown-paused.json";
import questionAndBudget from "../test/fixtures/operation-trace/question-and-budget-pause.json";
import verifiedSuccess from "../test/fixtures/operation-trace/verified-success.json";
import waitingApproval from "../test/fixtures/operation-trace/waiting-approval.json";
import { BUDGET_KINDS, budgetLabel, reverseEdgeLabel, waitText } from "./operationText";
import {
  EDGE_KINDS,
  EXECUTION_STATES,
  OPERATION_STATUSES,
  STEP_KINDS,
  STEP_OUTCOMES,
} from "./operationVocab";
import {
  edgeKindMeta,
  executionStateMeta,
  operationStatusMeta,
  riskMeta,
  stepKindMeta,
  stepOutcomeMeta,
} from "./tone";

const TRACES = [
  failedVerification,
  outcomeUnknown,
  questionAndBudget,
  verifiedSuccess,
  waitingApproval,
];

describe("operation vocabularies", () => {
  it("operation_vocabularies_cover_every_value_the_contract_names", () => {
    expect(Object.keys(OPERATION_STATUSES)).toHaveLength(11);
    expect(Object.keys(STEP_OUTCOMES)).toHaveLength(10);
    expect(Object.keys(EXECUTION_STATES)).toHaveLength(12);
    expect(Object.keys(STEP_KINDS)).toHaveLength(11);
    expect(Object.keys(EDGE_KINDS)).toHaveLength(5);
  });

  it("every_status_kind_outcome_and_edge_in_the_fixtures_has_words", () => {
    for (const trace of TRACES) {
      expect(OPERATION_STATUSES).toHaveProperty(trace.operation.status);
      for (const step of trace.steps) {
        expect(STEP_KINDS).toHaveProperty(step.kind);
        expect(STEP_OUTCOMES).toHaveProperty(step.outcome);
      }
      for (const edge of trace.edges) {
        expect(EDGE_KINDS).toHaveProperty(edge.kind);
      }
    }
  });

  it("unknown_operation_values_are_neutral_with_the_raw_value", () => {
    for (const meta of [
      operationStatusMeta,
      stepOutcomeMeta,
      executionStateMeta,
      stepKindMeta,
      edgeKindMeta,
    ]) {
      const m = meta("from_the_future");
      expect(m.tone).toBe("neutral");
      expect(m.label).toBe("from_the_future");
    }
  });

  it("unknown_risk_has_its_own_status", () => {
    const m = riskMeta("unknown");
    expect(m.tone).toBe("warning");
    expect(m.label).toBe("Risk unknown");
    expect(m.hint).toContain("treated as destructive");
  });

  it("operation_text_words_are_plain", () => {
    expect(BUDGET_KINDS).toHaveLength(7);
    expect(budgetLabel("executions")).toBe("Actions");
    expect(budgetLabel("wall_time")).toBe("Time");
    expect(budgetLabel("odd")).toBe("odd");
    expect(waitText("approval")).toBe("waiting for your approval");
    expect(waitText("agent")).toBe("waiting for the host's agent");
    expect(waitText("odd")).toBe("waiting");
    expect(reverseEdgeLabel("depends_on")).toBe("Needed by");
    expect(reverseEdgeLabel("odd")).toBe("odd");
  });
});
