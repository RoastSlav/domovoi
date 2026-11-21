import { screen } from "@testing-library/react";
import { skipAxe } from "../test/axe";
import { renderAt } from "../test/render";
import { PolicyDemos } from "./PolicyDemos";

describe("the policy catalogue demos", () => {
  it("policy_demos_render_every_state", () => {
    // A fragment catalogue: demo headings and repeated groups make heading levels jump by design.
    skipAxe();
    renderAt("/", "/", <PolicyDemos />);
    expect(screen.getByRole("heading", { name: "Decision error" })).toBeVisible();
    expect(screen.getAllByRole("group", { name: /enforced$/ })).toHaveLength(5);
    expect(screen.getByRole("button", { name: "Open PasswordPrompt" })).toBeVisible();
  });
});
