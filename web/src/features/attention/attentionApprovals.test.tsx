import { act, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { applyJournal } from "../../api/journalFrames";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { summary } from "../../test/attentionFixtures";
import { serveAttention, waitingDetail } from "../../test/attentionRoutes";
import { renderAt } from "../../test/render";
import { AttentionPage } from "./AttentionPage";

vi.mock("../../api/client", () => import("../../test/apiMock"));

beforeEach(() => {
  resetApi();
  localStorage.clear();
});

const open = (url = "/attention") => renderAt(url, "/attention", <AttentionPage />);
const waiting = (n: number) => summary(`op-${n}`, "waiting_approval", { goal: `Plan ${n}` });
const many = (count: number) => Array.from({ length: count }, (_, n) => waiting(n));
const detailsOf = (count: number) =>
  Object.fromEntries(
    Array.from({ length: count }, (_, n) => [`op-${n}`, waitingDetail(`op-${n}`, `Plan ${n}`)]),
  );

describe("attention approvals", () => {
  it("approvals_are_shared_cards_with_primary_approve_and_an_operation_link", async () => {
    serveAttention({
      operations: { waiting_approval: [waiting(0)] },
      details: detailsOf(1),
    });
    open();

    const card = await screen.findByRole("group", { name: "Action restart_container" });
    const approve = within(card).getByRole("button", { name: "Approve" });
    expect(approve.className, "the primary action on this page").toMatch(/primary/);
    expect(within(card).getByRole("link", { name: /Plan 0/ })).toHaveAttribute(
      "href",
      "/operations/op-0",
    );
  });

  it("an_approval_that_cannot_load_says_so_and_keeps_the_other_cards", async () => {
    serveAttention({
      operations: { waiting_approval: [waiting(0), waiting(1)] },
      details: { "op-1": waitingDetail("op-1", "Plan 1") },
    });
    route("GET /api/v1/operations/{id}", (init) =>
      init.params?.path?.id === "op-0"
        ? failure(500, "internal", "boom")
        : waitingDetail("op-1", "Plan 1"),
    );
    open();

    expect(await screen.findByText("Could not load Plan 0")).toBeVisible();
    expect(await screen.findByRole("group", { name: "Action restart_container" })).toBeVisible();
    expect(screen.getAllByRole("button", { name: "Retry" })).toHaveLength(1);
  });

  it("approval_cards_beyond_ten_are_revealed_by_show_more", async () => {
    serveAttention({ operations: { waiting_approval: many(12) }, details: detailsOf(12) });
    open();

    await vi.waitFor(() =>
      expect(screen.getAllByRole("group", { name: "Action restart_container" })).toHaveLength(10),
    );
    await userEvent.setup().click(screen.getByRole("button", { name: "Show 2 more" }));
    await vi.waitFor(() =>
      expect(screen.getAllByRole("group", { name: "Action restart_container" })).toHaveLength(12),
    );
    expect(screen.queryByRole("button", { name: /^Show \d+ more$/ })).toBeNull();
  });

  it("a_journal_frame_newer_than_the_page_refreshes_it", async () => {
    serveAttention();
    const { client } = open();
    await screen.findByText("Nothing needs you");
    const before = calls.filter((call) => call.route === "GET /api/v1/operations").length;

    serveAttention({ operations: { paused: [summary("late", "paused")] } });
    await act(() =>
      applyJournal(client, {
        seq: 5,
        aggregate_type: "operation",
        aggregate_id: "late",
        kind: "operation_paused",
        payload_v: 1,
        payload: null,
        actor: "system",
        at: "2026-01-01T00:00:00Z",
        topics: ["operations"],
      }),
    );

    expect(await screen.findByRole("link", { name: /Goal late/ })).toBeVisible();
    expect(calls.filter((c) => c.route === "GET /api/v1/operations").length).toBeGreaterThan(
      before,
    );
  });
});
