import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";
import { resetEngine } from "../../components/graph/testEngine";
import { resetApi } from "../../test/apiMock";
import { motionStub } from "../../test/motionStub";
import { operationDetail } from "../../test/operationFixtures";
import { openBar, openReplayPage } from "../../test/replayPage";
import { at } from "../../test/at";

vi.mock("../../api/client", () => import("../../test/apiMock"));
vi.mock("../../components/graph/GraphEngine", () => import("../../components/graph/testEngine"));
vi.mock("../../design/motion", async (original) =>
  (await import("../../test/motionStub")).stubbedMotion(await original()),
);

beforeEach(() => {
  resetApi();
  resetEngine();
  motionStub.ms = 240;
});

const detail = operationDetail("verified-success");
const first = at(detail.trace.steps, 0);
const inspector = () => screen.findByRole("complementary", { name: "Step inspector" });

test("the_inspector_says_it_shows_the_live_record_during_a_replay", async () => {
  const { user } = await openReplayPage(detail, { search: `?step=${first.id}` });
  const panel = await inspector();
  expect(within(panel).queryByText("Showing the live record")).toBeNull();

  await openBar(user);
  expect(within(panel).queryByText("Showing the live record")).toBeNull();

  act(() => void fireEvent.change(screen.getByRole("slider"), { target: { value: "1" } }));
  const label = (await screen.findByText(/^Replay: showing /)).textContent?.replace(
    "Replay: showing ",
    "",
  );
  const note = await within(panel).findByText("Showing the live record");
  expect(note.closest("[data-visibility-id]")).toHaveAttribute(
    "data-visibility-id",
    "inspector.replay-note",
  );
  expect(note.closest("[data-visibility-id]")).toHaveAttribute("data-visibility", "P0");
  expect(
    within(panel).getByText(`The graph shows ${label}. This panel shows the step as it is now.`),
  ).toBeVisible();
  // The note comes first, before the step's own facts.
  expect(panel.querySelector("[data-visibility-id]")).toBe(note.closest("[data-visibility-id]"));

  await user.click(screen.getByRole("button", { name: "Back to live" }));
  await waitFor(() => expect(within(panel).queryByText("Showing the live record")).toBeNull());
}, 20_000);
