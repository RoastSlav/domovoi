import { screen } from "@testing-library/react";
import { renderAt } from "../test/render";
import { SystemDemos } from "./SystemDemos";

describe("the settings, setup and sign-in catalogue demos", () => {
  it("draws a locked field with its reason, the stepper's current step and the auth card", () => {
    renderAt("/", "/", <SystemDemos />);
    const locked = screen.getByRole("textbox", { name: /Locked by hub\.toml/ });
    expect(locked).toHaveAttribute("readonly");
    expect(locked).toHaveAccessibleDescription(/Set in hub\.toml/);
    expect(screen.getByText(/^Model/).closest("li")).toHaveAttribute("aria-current", "step");
    expect(screen.getByRole("heading", { name: "Log in to Domovoi" })).toBeInTheDocument();
  });
});
