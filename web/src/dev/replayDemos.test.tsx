import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { REPLAY_TEXT } from "../features/operations/replay/replayText";
import { renderAt } from "../test/render";
import { ReplayDemos } from "./ReplayDemos";

const demo = (name: string): HTMLElement =>
  screen.getByRole("heading", { level: 3, name }).parentElement as HTMLElement;

describe("the replay demos", () => {
  it("replay_demos_render_without_violations", () => {
    renderAt("/", "/", <ReplayDemos />);
    expect(screen.getByRole("heading", { level: 2, name: "Replay" })).toBeVisible();
    expect(demo("Replay bar, at live")).toHaveTextContent("Live");
    expect(within(demo("Replay bar, replaying")).getByText(/^Replay: showing /)).toBeVisible();
    expect(
      within(demo("Replay bar, playing")).getByRole("button", { name: "Pause" }),
    ).toBeVisible();
    expect(
      within(demo("Replay bar, reduced motion")).getByRole("button", { name: "Play" }),
    ).toHaveAttribute("aria-disabled", "true");
    expect(
      within(demo("Replay bar, history failed")).getByRole("button", { name: "Retry" }),
    ).toBeVisible();
    expect(
      within(demo("Replay bar, oldest records removed")).getByText(REPLAY_TEXT.pruned),
    ).toBeVisible();
    expect(
      within(demo("Replay bar, latest records missing")).getByText(REPLAY_TEXT.capped),
    ).toBeVisible();
    expect(
      within(demo("Replay bar, nothing to replay")).getByText(REPLAY_TEXT.nothing),
    ).toBeVisible();
    // The setup's accessibility check runs over everything left on screen.
  });

  it("the_scrubber_demo_moves_with_the_keyboard", async () => {
    renderAt("/", "/", <ReplayDemos />);
    const slider = within(demo("Scrubber")).getByRole("slider", { name: "Position" });
    expect(slider).toHaveValue("3");
    slider.focus();
    await userEvent.setup().keyboard("{End}");
    expect(slider).toHaveValue("6");
    expect(
      within(demo("Scrubber")).getByRole("slider", { name: "Position (off)" }),
    ).toHaveAccessibleDescription("Nothing to move through yet.");
  });
});
