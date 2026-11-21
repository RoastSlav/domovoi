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

test("an_explanation_warns_when_part_of_the_decision_could_not_be_read", () => {
  const warning = "Part of this explanation could not be read";
  const { unmount } = render(
    <DecisionExplanation
      title="Active policy"
      decision={decisionOf({ outcome: { kind: "allow" }, facts_used: [1], extra: 1 })}
      zone={undefined}
    />,
  );
  expect(screen.getByText(warning)).toBeInTheDocument();
  unmount();
  render(
    <DecisionExplanation
      title="Active policy"
      decision={decisionOf(DECISION_LOCKED.enforced)}
      zone={undefined}
    />,
  );
  expect(screen.queryByText(warning)).not.toBeInTheDocument();
});
