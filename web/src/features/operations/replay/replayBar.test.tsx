import { useEffect } from "react";
import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { OperationHistory } from "../../../api/types";
import { ApiFailure } from "../../../api/types";
import { motionStub } from "../../../test/motionStub";
import { historyFor, historyOf, ev, must, TRACE_FIXTURES } from "../../../test/replayFixtures";
import { formatClock } from "./clock";
import { ReplayBar, type HistoryLoad } from "./ReplayBar";
import { REPLAY_TEXT } from "./replayText";
import { usePlayback, type Playback } from "./usePlayback";

vi.mock("../../../design/motion", async (original) =>
  (await import("../../../test/motionStub")).stubbedMotion(await original()),
);

const trace = must(TRACE_FIXTURES["verified-success"]);
const full = historyFor(trace);

let latest: Playback | undefined;
const playback = (): Playback => latest as Playback;

function Bar({
  history,
  load,
  reason,
}: {
  history: OperationHistory;
  load: HistoryLoad;
  reason?: string | null;
}) {
  const current = usePlayback(trace, history);
  useEffect(() => {
    latest = current;
  });
  return (
    <ReplayBar
      playback={current}
      trace={trace}
      load={load}
      reason={reason === undefined ? history.incomplete_reason : reason}
    />
  );
}

function show(history: OperationHistory = full, load: HistoryLoad = { kind: "ready" }) {
  return render(<Bar history={history} load={load} />);
}

beforeEach(() => {
  motionStub.ms = 240;
});
afterEach(() => {
  latest = undefined;
  vi.useRealTimers();
});

const byName = (name: string) => screen.getByRole("button", { name });
const reasonOf = (el: HTMLElement) => el.getAttribute("aria-description");
const scrubber = () => screen.getByRole("slider", { name: "Replay position" });
const status = () => screen.getAllByRole("status").find((el) => el.tagName === "P") as HTMLElement;

describe("the replay bar", () => {
  it("opens_at_live_with_every_control_and_the_words_of_the_brief", () => {
    show();
    const group = screen.getByRole("group", { name: "Replay" });
    expect(within(group).getByRole("button", { name: "Previous moment" })).toBeVisible();
    expect(within(group).getByRole("button", { name: "Next moment" })).toBeVisible();
    expect(within(group).getByRole("button", { name: "Play" })).toBeVisible();
    expect(within(group).getByRole("button", { name: "Back to live" })).toBeVisible();
    const speeds = within(screen.getByRole("group", { name: "Speed" })).getAllByRole("radio");
    expect(speeds.map((radio) => radio.closest("label")?.textContent)).toEqual(["1×", "2×", "4×"]);
    expect(status()).toHaveTextContent(/^Live$/);
    expect(scrubber()).toHaveAttribute("max", String(playback().positions.length - 1));
    expect(scrubber()).toHaveValue(String(playback().positions.length - 1));
    expect(screen.queryByText(/^Replay: showing/)).toBeNull();
  });

  it("previous_and_next_have_their_disabled_reasons", async () => {
    show();
    expect(byName("Next moment")).toHaveAttribute("aria-disabled", "true");
    expect(reasonOf(byName("Next moment"))).toBe("Already showing the live state.");
    expect(reasonOf(byName("Back to live"))).toBe("Already showing the live state.");
    await userEvent.setup().click(scrubber());
    act(() => playback().seek(0));
    expect(reasonOf(byName("Previous moment"))).toBe("Already at the first moment.");
    expect(byName("Next moment")).not.toHaveAttribute("aria-disabled");
    expect(byName("Back to live")).not.toHaveAttribute("aria-disabled");
  });

  it("play_is_disabled_with_its_reason_under_reduced_motion", () => {
    motionStub.ms = 0;
    show();
    expect(byName("Play")).toHaveAttribute("aria-disabled", "true");
    expect(reasonOf(byName("Play"))).toBe(REPLAY_TEXT.reduced);
    expect(REPLAY_TEXT.reduced).toBe(
      "Automatic replay is off because your system asks for reduced motion. Use Previous moment and Next moment.",
    );
  });

  it("the_scrubber_announces_the_moment_and_live", () => {
    show();
    const count = playback().positions.length - 1;
    expect(scrubber()).toHaveAttribute("aria-valuetext", "Live");
    act(() => playback().seek(1));
    const frame = playback().frame;
    const clock = formatClock((frame as { atMs: number }).atMs);
    expect(scrubber()).toHaveAttribute("aria-valuetext", `Moment 2 of ${count}, ${clock}`);
    expect(status()).toHaveTextContent(new RegExp(`^Moment 2 of ${count}: ${clock}\\. .+`));
  });

  it("the_status_line_is_polite_unless_playing_and_the_replay_note_names_the_moment", () => {
    show();
    expect(status()).toHaveAttribute("aria-live", "polite");
    act(() => playback().seek(0));
    const clock = formatClock((playback().frame as { atMs: number }).atMs);
    const note = screen.getByText(`Replay: showing ${clock}`).closest("[data-visibility-id]");
    expect(note).toHaveAttribute("data-visibility", "P0");
    expect(note).toHaveAttribute("data-visibility-id", "operation.replay-note");
    expect(screen.getByText(REPLAY_TEXT.noteBody)).toBeVisible();
    act(() => playback().play());
    expect(playback().state.playing).toBe(true);
    expect(status()).toHaveAttribute("aria-live", "off");
    expect(byName("Pause")).toBeVisible();
    act(() => playback().pause());
    expect(byName("Play")).toBeVisible();
  });

  it("the_scrubber_moves_the_position_and_back_to_live_returns", async () => {
    show();
    const user = userEvent.setup();
    scrubber().focus();
    await user.keyboard("{Home}");
    expect(playback().index).toBe(0);
    await user.keyboard("{ArrowRight}");
    expect(playback().index).toBe(1);
    await user.click(byName("Next moment"));
    expect(playback().index).toBe(2);
    await user.click(byName("Previous moment"));
    expect(playback().index).toBe(1);
    await user.click(byName("Back to live"));
    expect(playback().state.position).toEqual({ kind: "live" });
  });

  it("speed_choices_change_the_speed", async () => {
    show();
    await userEvent.setup().click(screen.getByRole("radio", { name: "4×" }));
    expect(playback().state.speed).toBe(4);
  });

  it.each([
    ["pruned", [REPLAY_TEXT.pruned], [REPLAY_TEXT.capped]],
    ["capped", [REPLAY_TEXT.capped], [REPLAY_TEXT.pruned]],
    ["both", [REPLAY_TEXT.pruned, REPLAY_TEXT.capped], []],
  ])("incomplete_history_says_so (%s)", (reason, shown, hidden) => {
    show({ ...full, complete: false, incomplete_reason: reason });
    for (const sentence of shown) {
      const note = screen.getByText(sentence);
      expect(note.closest("[data-visibility-id]")).toHaveAttribute(
        "data-visibility-id",
        "operation.replay-gap",
      );
    }
    for (const sentence of hidden) {
      expect(screen.queryByText(sentence)).toBeNull();
    }
  });

  it("a_capped_history_ends_with_live_appended_not_replacing_its_last_moment", () => {
    const capped = { ...full, complete: false, incomplete_reason: "capped" };
    show(capped);
    const frames = playback().frames.length;
    expect(playback().positions).toHaveLength(frames + 1);
  });

  it("fewer_than_two_positions_shows_nothing_to_replay_yet", () => {
    show(historyOf([ev(1, "2026-01-01T00:00:00Z", "operation.created")]));
    expect(screen.getByText("Nothing to replay yet.")).toBeVisible();
    expect(screen.queryByRole("slider")).toBeNull();
    expect(screen.queryByRole("button", { name: "Play" })).toBeNull();
    expect(byName("Back to live")).toHaveAttribute("aria-disabled", "true");
  });

  it("history_loading_shows_a_placeholder_and_no_controls", () => {
    const { container } = show(full, { kind: "loading" });
    expect(container.querySelector('[aria-hidden="true"]')).not.toBeNull();
    expect(screen.queryByRole("group", { name: "Replay" })).toBeNull();
  });

  it("history_failure_shows_a_load_error_with_retry", async () => {
    const retry = vi.fn();
    show(full, {
      kind: "failed",
      error: new ApiFailure(500, "internal", "boom"),
      retry,
    });
    expect(screen.getByRole("alert")).toHaveTextContent(/load the history/i);
    await userEvent.setup().click(screen.getByRole("button", { name: "Retry" }));
    expect(retry).toHaveBeenCalledOnce();
  });
});
