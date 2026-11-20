import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { skipAxe } from "../test/axe";
import { renderAt } from "../test/render";
import { NewKitDemos } from "./NewKitDemos";

describe("the stat tile, palette and clickable row catalogue demos", () => {
  it("shows the tiles, runs a palette choice and reports a row click", async () => {
    // The catalogue repeats demo headings and fragments, so heading levels jump by design.
    skipAxe();
    const user = userEvent.setup();
    renderAt("/", "/", <NewKitDemos />);
    expect(screen.getByRole("link", { name: /Waiting for approval/ })).toHaveAttribute(
      "href",
      "/dev/design",
    );
    expect(screen.getByText("3 of 4")).toBeVisible();

    await user.click(screen.getByRole("button", { name: "Search…" }));
    await user.type(await screen.findByRole("combobox", { name: "Search" }), "dark");
    await user.keyboard("{Enter}");
    expect(screen.getByText("Toggle theme")).toBeVisible();

    await user.click(screen.getByText("2 hours ago"));
    expect(screen.getByText("Clicked pi-garage")).toBeVisible();
  });
});

describe("the Operator thread catalogue demos", () => {
  it("shows a failed answer with Retry and a pending one with Stop", () => {
    skipAxe();
    renderAt("/", "/", <NewKitDemos />);
    expect(screen.getByText(/monthly model budget is spent/)).toBeVisible();
    expect(screen.getByRole("button", { name: "Retry" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Stop" })).toBeVisible();
    expect(screen.getByText(/Nothing needs you today/)).toBeVisible();
  });
});
