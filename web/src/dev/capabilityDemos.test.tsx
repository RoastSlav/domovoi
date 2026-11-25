import { screen } from "@testing-library/react";
import { skipAxe } from "../test/axe";
import { renderAt } from "../test/render";
import { CapabilityDemos } from "./CapabilityDemos";

describe("the capability catalogue demos", () => {
  it("capability_demos_render_the_limits_table_and_the_helper_views", () => {
    // A fragment catalogue: several helper cards on one page repeat their headings by design.
    skipAxe();
    renderAt("/", "/", <CapabilityDemos />);
    expect(screen.getByRole("table", { name: "Unattended limits reported by nas" })).toBeVisible();
    expect(screen.getByText("Helper key changed: re-enrol")).toBeVisible();
    expect(screen.getAllByText("Verified")).not.toHaveLength(0);
  });
});
