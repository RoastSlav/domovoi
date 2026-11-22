import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { OperationHistory, OperationDetail } from "../api/types";
import { calls, route } from "./apiMock";
import { openGraphPage } from "./graphPage";
import { historyFor } from "./replayFixtures";

/** The history route of the operation page. */
export const HISTORY = "GET /api/v1/operations/{id}/history";

/** How many times the page asked for the history. */
export const historyRequests = (): number => calls.filter((call) => call.route === HISTORY).length;

/** The button that opens and closes the replay bar. */
export const replayToggle = (): HTMLElement =>
  screen.getByRole("button", { name: /^(Replay|Close replay)$/ });

/**
 * Renders the operation page in the Graph view with a history route and waits for the graph.
 * By default the history is the one consistent with the trace.
 */
export async function openReplayPage(
  detail: OperationDetail,
  options: Parameters<typeof openGraphPage>[1] & {
    history?: OperationHistory | (() => unknown);
  } = {},
) {
  const { history = historyFor(detail.trace), ...rest } = options;
  route(HISTORY, typeof history === "function" ? history : () => history);
  const page = await openGraphPage(detail, rest);
  return { ...page, user: userEvent.setup() };
}

/** Opens the bar and waits until the history has arrived. */
export async function openBar(user: ReturnType<typeof userEvent.setup>) {
  await user.click(replayToggle());
  return screen.findByRole("group", { name: "Replay" });
}
