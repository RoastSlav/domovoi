import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { skipAxe } from "../test/axe";
import { renderAt } from "../test/render";
import { KnowledgeDemos } from "./KnowledgeDemos";

describe("the knowledge catalogue demos", () => {
  it("knowledge_demos_render_in_every_state", async () => {
    // A fragment catalogue: demo headings and repeated groups make heading levels jump by design.
    skipAxe();
    const user = userEvent.setup();
    renderAt("/", "/", <KnowledgeDemos />);
    expect(screen.getByRole("table", { name: "Facts in every status" })).toBeVisible();
    expect(screen.getByText("Unattended containment is blocked on nas")).toBeVisible();
    expect(screen.getByText("Management paths are confirmed and current on nas")).toBeVisible();
    expect(screen.getAllByText("Cannot be verified yet").length).toBeGreaterThan(0);
    await user.click(screen.getByRole("button", { name: "Open with wrong password" }));
    expect(await screen.findByText("That password is not right.")).toBeVisible();
  });
});
