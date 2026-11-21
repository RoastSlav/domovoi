import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { skipAxe } from "../test/axe";
import { renderAt } from "../test/render";
import { VisibilityDemos } from "./VisibilityDemos";

test("visibility_demos_render_every_primitive_in_every_profile", async () => {
  // The catalogue repeats demo headings, so heading levels jump by design.
  skipAxe();
  const user = userEvent.setup();
  renderAt("/", "/", <VisibilityDemos />);
  expect(screen.getAllByText("Waiting for your approval.")).toHaveLength(3);
  expect(screen.getAllByText("exit 0, 3 lines")).toHaveLength(1);
  expect(screen.getAllByRole("button", { name: /Step output/ })).toHaveLength(2);
  expect(screen.getByRole("button", { name: "+2 details" })).toBeVisible();
  await user.click(screen.getByRole("button", { name: /Connections/ }));
  expect(screen.getByText("Three linked steps.")).toBeVisible();
});
