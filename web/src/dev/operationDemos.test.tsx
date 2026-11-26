import { screen } from "@testing-library/react";
import { skipAxe } from "../test/axe";
import { renderAt } from "../test/render";
import { OperationDemos } from "./OperationDemos";

describe("the operations catalogue demos", () => {
  it("operation_demos_render_in_every_state", () => {
    // The catalogue repeats demo headings and fragments, so heading levels jump by design.
    skipAxe();
    renderAt("/", "/", <OperationDemos />);
    expect(
      screen.getAllByRole("group", { name: "Action restart_container" }).length,
    ).toBeGreaterThan(8);
    expect(screen.getByText("Risk unknown")).toBeVisible();
    expect(screen.getAllByRole("button", { name: /Close Step inspector/ })).toHaveLength(2);
  });
});
