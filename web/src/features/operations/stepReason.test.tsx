import { render, screen } from "@testing-library/react";
import { StepReason } from "./InspectorSections";

test("a_reason_is_shown_for_an_unfamiliar_outcome_because_it_is_treated_as_a_problem", () => {
  render(<StepReason reason="The agent went away." outcome="from_the_future" />);
  expect(screen.getByText("The agent went away.")).toBeVisible();
});

test("a_reason_is_not_shown_for_a_known_outcome_that_is_not_a_problem", () => {
  render(<StepReason reason="All good." outcome="succeeded" />);
  expect(screen.queryByText("All good.")).toBeNull();
});
