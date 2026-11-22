import { screen } from "@testing-library/react";
import { skipAxe } from "../test/axe";
import { renderAt } from "../test/render";
import { AttentionDemos } from "./AttentionDemos";

describe("the attention catalogue demos", () => {
  it("attention_demos_render_every_kind_of_row_in_its_tier", () => {
    // A catalogue of fragments: the headings repeat and jump levels by design.
    skipAxe();
    renderAt("/", "/", <AttentionDemos />);

    expect(screen.getAllByRole("heading", { level: 4 }).map((h) => h.textContent)).toEqual([
      "Act now",
      "Waiting for you",
      "Worth a look",
    ]);
    expect(screen.getByRole("link", { name: /Incompatible.*old-agent/ })).toBeVisible();
    expect(screen.getByRole("link", { name: /Offline.*garage/ })).toBeVisible();
    expect(screen.getByText("A limit is spent. Raise it to continue, or cancel it.")).toBeVisible();
    expect(screen.getByText("It paused after an error.")).toBeVisible();
    expect(screen.getByRole("link", { name: /mystery/ })).toBeVisible();
    expect(screen.getByText("Could not load incidents and paused operations")).toBeVisible();
    expect(screen.getByRole("button", { name: "Retry" })).toBeVisible();
  });
});
