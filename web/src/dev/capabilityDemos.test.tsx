import { screen } from "@testing-library/react";
import { skipAxe } from "../test/axe";
import { renderAt } from "../test/render";
import { CapabilityDemos } from "./CapabilityDemos";

describe("the capability catalogue demos", () => {
  it("capability_demos_render_the_limits_table_and_both_placeholders", () => {
    // A fragment catalogue: two placeholders on one page repeat their headings by design.
    skipAxe();
    renderAt("/", "/", <CapabilityDemos />);
    expect(screen.getByRole("table", { name: "Unattended limits reported by nas" })).toBeVisible();
    expect(screen.getAllByText("Not available on this hub yet")).toHaveLength(2);
  });
});
