import { render, screen } from "@testing-library/react";
import { DECISION_ERROR, DECISION_LOCKED } from "../../test/policyFixtures";
import { DecisionExplanation } from "./decision/DecisionExplanation";
import { decisionOf } from "./decisionModel";

test("an_explanation_of_a_missing_decision_renders_nothing", () => {
  const { container } = render(<DecisionExplanation title="x" decision={null} zone={undefined} />);
  expect(container).toBeEmptyDOMElement();
});

test("an_explanation_of_a_decision_with_no_match_says_the_empty_text", () => {
  render(
    <DecisionExplanation
      title="Shadow rules"
      decision={decisionOf(DECISION_LOCKED.shadow)}
      zone={undefined}
      emptyText="Nothing matched."
    />,
  );
  expect(screen.getByText("Nothing matched.")).toBeInTheDocument();
});

test("an_explanation_of_an_error_is_one_warning_with_its_hint", () => {
  render(
    <DecisionExplanation
      title="Active policy"
      decision={decisionOf(DECISION_ERROR.enforced)}
      zone={undefined}
    />,
  );
  expect(screen.getAllByRole("status")).toHaveLength(1);
});
