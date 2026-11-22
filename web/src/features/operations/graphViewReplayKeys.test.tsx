import { act, fireEvent, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { resetEngine } from "../../components/graph/testEngine";
import { resetApi } from "../../test/apiMock";
import { graphRoot } from "../../test/graphPage";
import { motionStub } from "../../test/motionStub";
import { operationDetail } from "../../test/operationFixtures";
import { historyRequests, openBar, openReplayPage } from "../../test/replayPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));
vi.mock("../../components/graph/GraphEngine", () => import("../../components/graph/testEngine"));
vi.mock("../../design/motion", async (original) =>
  (await import("../../test/motionStub")).stubbedMotion(await original()),
);

const SLOW = 20_000;
const detail = operationDetail("verified-success");

beforeEach(() => {
  resetApi();
  resetEngine();
  motionStub.ms = 240;
});

const slider = () => screen.getByRole("slider", { name: "Replay position" });
const root = () => graphRoot() as HTMLElement;
const press = (key: string, init: KeyboardEventInit = {}, target: HTMLElement = root()) =>
  fireEvent.keyDown(target, { key, ...init });

describe("the replay keys on the graph", () => {
  it(
    "graph_keys_comma_period_space_and_l_work_only_with_the_bar_open",
    async () => {
      const { user } = await openReplayPage(detail);
      // Closed: not prevented, nothing happens, nothing is fetched.
      for (const key of [",", ".", " ", "l"]) {
        expect(press(key), `"${key}" with the bar closed`).toBe(true);
      }
      expect(historyRequests()).toBe(0);

      await openBar(user);
      const last = Number(slider().getAttribute("max"));
      act(() => void fireEvent.change(slider(), { target: { value: "3" } }));
      expect(press(",")).toBe(false);
      expect(slider()).toHaveValue("2");
      expect(press(".")).toBe(false);
      expect(slider()).toHaveValue("3");

      // A modifier, or a press that did not start on the graph's root, is left alone.
      expect(press(".", { ctrlKey: true })).toBe(true);
      expect(press(".", { metaKey: true })).toBe(true);
      expect(press(".", { altKey: true })).toBe(true);
      expect(press(".", { shiftKey: true })).toBe(true);
      expect(press(".", {}, root().firstElementChild as HTMLElement)).toBe(true);
      expect(slider()).toHaveValue("3");

      expect(press(" ")).toBe(false);
      expect(screen.getByRole("button", { name: "Pause" })).toBeVisible();
      expect(press(" ")).toBe(false);
      expect(screen.getByRole("button", { name: "Play" })).toBeVisible();

      expect(press("l")).toBe(false);
      expect(slider()).toHaveValue(String(last));
    },
    SLOW,
  );

  it(
    "the_graphs_own_keys_are_not_taken_by_the_replay",
    async () => {
      const { user } = await openReplayPage(detail);
      await openBar(user);
      act(() => void fireEvent.change(slider(), { target: { value: "3" } }));
      for (const key of ["ArrowRight", "Home", "End", "PageDown", "+", "-", "0", "a"]) {
        press(key);
      }
      expect(slider()).toHaveValue("3");
      expect(screen.getByRole("button", { name: "Play" })).toBeVisible();
    },
    SLOW,
  );
});
