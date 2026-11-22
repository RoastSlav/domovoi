import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { resetEngine } from "../../components/graph/testEngine";
import { failure, resetApi } from "../../test/apiMock";
import { address, drawnNodes, graphRoot } from "../../test/graphPage";
import { motionStub } from "../../test/motionStub";
import { operationDetail } from "../../test/operationFixtures";
import { expectP0Everywhere } from "../../test/visibility";
import type { HistoryEvent } from "../../api/types";
import { historyFor, historyOf, ev } from "../../test/replayFixtures";
import { historyRequests, openBar, openReplayPage, replayToggle } from "../../test/replayPage";
import { keys } from "../../api/queryClient";
import { FRAME_INTERVAL_MS } from "./replay/playback";

vi.mock("../../api/client", () => import("../../test/apiMock"));
vi.mock("../../components/graph/GraphEngine", () => import("../../components/graph/testEngine"));
vi.mock("../../design/motion", async (original) =>
  (await import("../../test/motionStub")).stubbedMotion(await original()),
);

const SLOW = 20_000;
const detail = operationDetail("verified-success");
const total = detail.trace.steps.length;

beforeEach(() => {
  resetApi();
  resetEngine();
  motionStub.ms = 240;
});
afterEach(() => vi.useRealTimers());

const slider = () => screen.getByRole("slider", { name: "Replay position" });
const frame = () => document.querySelector<HTMLElement>("[data-graph-frame]") as HTMLElement;
// The first moment has no steps yet, and then the graph shows its empty state instead.
const nodes = () => {
  const root = graphRoot();
  return root ? drawnNodes(root) : [];
};
const status = () => screen.getAllByRole("status").find((el) => el.tagName === "P") as HTMLElement;
const goTo = (index: number) => fireEvent.change(slider(), { target: { value: String(index) } });

describe("replay in the Graph view", () => {
  it(
    "replay_is_closed_by_default_and_fetches_nothing",
    async () => {
      await openReplayPage(detail);
      expect(replayToggle()).toHaveTextContent("Replay");
      expect(screen.queryByRole("group", { name: "Replay" })).toBeNull();
      expect(historyRequests()).toBe(0);
    },
    SLOW,
  );

  it(
    "opening_replay_fetches_the_history_and_shows_the_bar_at_live",
    async () => {
      const { user } = await openReplayPage(detail);
      await openBar(user);
      expect(historyRequests()).toBe(1);
      expect(replayToggle()).toHaveTextContent("Close replay");
      expect(status()).toHaveTextContent(/^Live$/);
      expect(nodes()).toHaveLength(total);
      expect(frame()).not.toHaveAttribute("data-replay");
      expect(replayToggle().closest("[data-visibility-id]")).toHaveAttribute(
        "data-visibility",
        "P1",
      );
    },
    SLOW,
  );

  it(
    "moving_to_an_earlier_moment_draws_the_past_and_shows_the_replay_note",
    async () => {
      const { user } = await openReplayPage(detail);
      await openBar(user);
      act(() => goTo(1));
      await waitFor(() => expect(nodes().length).toBeLessThan(total));
      expect(screen.getByText(/^Replay: showing /)).toBeVisible();
      expect(frame()).toHaveAttribute("data-replay", "true");
      await user.click(screen.getByRole("button", { name: "Next moment" }));
      expect(nodes().length).toBeGreaterThanOrEqual(1);
      expect(historyRequests()).toBe(1);
    },
    SLOW,
  );

  it(
    "back_to_live_restores_the_live_graph_and_removes_the_note",
    async () => {
      const { user } = await openReplayPage(detail);
      await openBar(user);
      act(() => goTo(0));
      await screen.findByText(/^Replay: showing /);
      await user.click(screen.getByRole("button", { name: "Back to live" }));
      await waitFor(() => expect(nodes()).toHaveLength(total));
      expect(screen.queryByText(/^Replay: showing /)).toBeNull();
      expect(frame()).not.toHaveAttribute("data-replay");
    },
    SLOW,
  );

  it(
    "the_step_list_and_waiting_for_you_stay_live_during_a_replay",
    async () => {
      const waiting = operationDetail("waiting-approval");
      const { user } = await openReplayPage(waiting);
      await openBar(user);
      act(() => goTo(0));
      await screen.findByText(/^Replay: showing /);
      expect(nodes().length).toBeLessThan(waiting.trace.steps.length);
      const lists = screen.getAllByRole("list", { name: /^(Steps|Cycle \d+)$/ });
      const rows = lists.reduce((n, list) => n + within(list).getAllByRole("listitem").length, 0);
      expect(rows).toBe(waiting.trace.steps.length);
      expect(screen.getAllByRole("button", { name: "Approve" }).length).toBeGreaterThan(0);
    },
    SLOW,
  );

  it(
    "play_is_disabled_with_its_reason_under_reduced_motion",
    async () => {
      motionStub.ms = 0;
      const { user } = await openReplayPage(detail);
      await openBar(user);
      const play = screen.getByRole("button", { name: "Play" });
      expect(play).toHaveAttribute("aria-disabled", "true");
      await user.click(play);
      expect(slider()).toHaveValue(slider().getAttribute("max"));
      await user.click(screen.getByRole("button", { name: "Previous moment" }));
      await waitFor(() => expect(nodes().length).toBeLessThanOrEqual(total));
      expect(screen.getByText(/^Replay: showing /)).toBeVisible();
    },
    SLOW,
  );

  it(
    "play_advances_and_pause_stops",
    async () => {
      const { user } = await openReplayPage(detail);
      await openBar(user);
      act(() => goTo(0));
      vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
      fireEvent.click(screen.getByRole("button", { name: "Play" }));
      expect(status()).toHaveAttribute("aria-live", "off");
      act(() => void vi.advanceTimersByTime(FRAME_INTERVAL_MS));
      expect(slider()).toHaveValue("1");
      fireEvent.click(screen.getByRole("button", { name: "Pause" }));
      expect(status()).toHaveAttribute("aria-live", "polite");
      act(() => void vi.advanceTimersByTime(FRAME_INTERVAL_MS * 3));
      expect(slider()).toHaveValue("1");
    },
    SLOW,
  );

  it(
    "history_failure_shows_a_load_error_with_retry_and_leaves_the_graph_live",
    async () => {
      let broken = true;
      const { user } = await openReplayPage(detail, {
        history: () => (broken ? failure(500, "internal", "boom") : historyFor(detail.trace)),
      });
      await user.click(replayToggle());
      expect(await screen.findByText(/load the history/i)).toBeVisible();
      expect(nodes()).toHaveLength(total);
      broken = false;
      await user.click(screen.getByRole("button", { name: "Retry" }));
      expect(await screen.findByRole("group", { name: "Replay" })).toBeVisible();
    },
    SLOW,
  );

  it(
    "fewer_than_two_positions_shows_nothing_to_replay_yet",
    async () => {
      const created = detail.operation.created_at;
      const { user } = await openReplayPage(detail, {
        history: historyOf([ev(1, created, "operation.created")]),
      });
      await openBar(user);
      expect(screen.getByText("Nothing to replay yet.")).toBeVisible();
      expect(nodes()).toHaveLength(total);
    },
    SLOW,
  );

  it(
    "new_history_while_replaying_keeps_the_moment",
    async () => {
      const base = historyFor(detail.trace);
      let events: HistoryEvent[] = base.events;
      const { user, client } = await openReplayPage(detail, {
        history: () => historyOf(events),
      });
      await openBar(user);
      act(() => goTo(2));
      const before = screen.getByText(/^Replay: showing /).textContent;
      const last = events[events.length - 1] as HistoryEvent;
      events = [
        ...events,
        ev(last.seq + 1, "2030-01-01T00:00:00Z", "operation.status", null, "running"),
      ];
      await act(async () => {
        await client.invalidateQueries({ queryKey: keys.operationHistory(detail.operation.id) });
      });
      await waitFor(() => expect(historyRequests()).toBe(2));
      expect(slider()).toHaveValue("2");
      expect(screen.getByText(/^Replay: showing /).textContent).toBe(before);
      expect(Number(slider().getAttribute("max"))).toBeGreaterThan(
        historyFor(detail.trace).events.length > 0 ? 2 : 0,
      );
    },
    SLOW,
  );

  it(
    "selecting_a_step_not_yet_created_keeps_the_url_and_clears_the_graph_selection",
    async () => {
      const later = detail.trace.steps[detail.trace.steps.length - 1]?.id as string;
      const { user, root } = await openReplayPage(detail, { search: `?step=${later}` });
      await waitFor(() => expect(root).toHaveAttribute("data-selected-step", later));
      await openBar(user);
      act(() => goTo(1));
      await screen.findByText(/^Replay: showing /);
      expect(nodes()).not.toContain(later);
      expect(address()).toContain(`?step=${later}`);
      expect(graphRoot()).toHaveAttribute("data-selected-step", "");
    },
    SLOW,
  );

  it(
    "replay_status_is_cleared_when_the_view_changes",
    async () => {
      const { user } = await openReplayPage(detail);
      await openBar(user);
      act(() => goTo(0));
      await screen.findByText(/^Replay: showing /);
      await user.click(screen.getByRole("radio", { name: "Timeline" }));
      expect(graphRoot()).toBeNull();
      expect(screen.queryByText(/^Replay: showing /)).toBeNull();
      await user.click(screen.getByRole("radio", { name: "Graph" }));
      await waitFor(() => expect(graphRoot()).not.toBeNull());
      expect(replayToggle()).toHaveTextContent("Replay");
      expect(screen.queryByRole("group", { name: "Replay" })).toBeNull();
      expect(nodes()).toHaveLength(total);
    },
    SLOW,
  );
});

describe("the replay notes in every detail level", () => {
  it("replay_and_gap_notes_are_visible_in_every_profile", async () => {
    const pruned = { ...historyFor(detail.trace), complete: false, incomplete_reason: "pruned" };
    await expectP0Everywhere({
      expectedIds: ["operation.replay-note", "operation.replay-gap"],
      render: async (profile) => {
        resetApi();
        const view = await openReplayPage(detail, { profile, history: pruned });
        await openBar(view.user);
        act(() => goTo(0));
        await screen.findByText(/^Replay: showing /);
        return view;
      },
    });
  }, 60_000);
});
