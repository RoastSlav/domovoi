import { render } from "@testing-library/react";
import { expectNoA11yViolations } from "./axe";

describe("the accessibility check", () => {
  it("fails on an image without a text alternative", async () => {
    const { container } = render(<img src="/logo.png" />);
    await expect(expectNoA11yViolations(container)).rejects.toThrow("image-alt");
  });

  it("passes a labelled control", async () => {
    const { container } = render(
      <label>
        Host
        <input type="text" />
      </label>,
    );
    await expectNoA11yViolations(container);
  });
});
