import { QueryClient, QueryClientProvider, type InfiniteData } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import { createElement, type ReactNode } from "react";
import { calls, failure, resetApi, route } from "../../test/apiMock";
import { detail, incident } from "../../test/fixtures";
import { keys } from "../queryClient";
import type { IncidentDetail, IncidentPage } from "../types";
import { useAck, useAsk } from "./incidents";
import { useDeleteNote } from "./notes";
import { PROFILE_POLL_MS, useProfile } from "./profile";

vi.mock("../client", () => import("../../test/apiMock"));

const ACK = "POST /api/v1/incidents/{id}/ack";
const OPEN_LIST = keys.incidentList({ status: "open" });

function setup() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const wrapper = ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client }, children);
  return { client, wrapper };
}

/** A client that already holds incident `inc-1`: its page, the open list and its follow-ups. */
function withIncident() {
  const made = setup();
  made.client.setQueryData<IncidentDetail>(keys.incident("inc-1"), detail());
  made.client.setQueryData<InfiniteData<IncidentPage>>(OPEN_LIST, {
    pages: [{ items: [incident(), incident({ id: "inc-2" })], next_cursor: null }],
    pageParams: [undefined],
  });
  made.client.setQueryData(keys.followUps("inc-1"), { turns: [] });
  return made;
}

const invalidated = (client: QueryClient, key: readonly unknown[]) =>
  client.getQueryState(key)?.isInvalidated;
const detailStatus = (client: QueryClient) =>
  client.getQueryData<IncidentDetail>(keys.incident("inc-1"))?.incident.status;
const listStatuses = (client: QueryClient) =>
  client
    .getQueryData<InfiniteData<IncidentPage>>(OPEN_LIST)
    ?.pages.flatMap((page) => page.items.map((item) => item.status));

beforeEach(resetApi);

afterEach(() => {
  vi.useRealTimers();
});

test("acking_refreshes_the_incident_the_lists_and_the_counts_but_not_the_follow_ups", async () => {
  const { client, wrapper } = withIncident();
  const invalidate = vi.spyOn(client, "invalidateQueries");
  route(ACK, () => undefined);
  const { result } = renderHook(() => useAck("inc-1"), { wrapper });

  await act(() => result.current.mutateAsync());

  expect(invalidate.mock.calls.map(([filters]) => filters?.queryKey)).toEqual([
    ["incidents", "detail", "inc-1"],
    ["incidents", "list"],
    ["overview"],
  ]);
  expect(invalidated(client, keys.incident("inc-1"))).toBe(true);
  expect(invalidated(client, OPEN_LIST)).toBe(true);
  expect(invalidated(client, keys.followUps("inc-1")), "the conversation did not change").toBe(
    false,
  );
});

test("an_ack_reads_as_acknowledged_before_the_hub_answers", async () => {
  const { client, wrapper } = withIncident();
  let during: { detail: string | undefined; list: string[] | undefined } | undefined;
  route(ACK, () => {
    during = { detail: detailStatus(client), list: listStatuses(client) };
    return undefined;
  });
  const { result } = renderHook(() => useAck("inc-1"), { wrapper });

  await act(() => result.current.mutateAsync());

  expect(during).toEqual({ detail: "acked", list: ["acked", "open"] });
});

test("a_failed_optimistic_ack_rolls_back", async () => {
  const { client, wrapper } = withIncident();
  let during: string | undefined;
  route(ACK, () => {
    during = detailStatus(client);
    return failure(409, "conflict", "the incident is already resolved");
  });
  const { result } = renderHook(() => useAck("inc-1"), { wrapper });

  await act(async () => {
    await result.current.mutateAsync().catch(() => undefined);
  });

  expect(during, "it read as acknowledged while the request ran").toBe("acked");
  expect(detailStatus(client)).toBe("open");
  expect(listStatuses(client)).toEqual(["open", "open"]);
  expect(result.current.isError).toBe(true);
});

test("a_follow_up_answer_refreshes_the_conversation_and_the_incidents_timeline", async () => {
  const { client, wrapper } = withIncident();
  route("POST /api/v1/incidents/{id}/ask", () => ({ answer: "It is the backup job." }));
  const { result } = renderHook(() => useAsk("inc-1"), { wrapper });

  await act(() => result.current.mutateAsync("why?"));

  expect(invalidated(client, keys.followUps("inc-1"))).toBe(true);
  expect(invalidated(client, keys.incident("inc-1"))).toBe(true);
  expect(invalidated(client, OPEN_LIST), "the lists do not show the timeline").toBe(false);
});

test("deleting_a_note_removes_it_from_the_cache", async () => {
  const { client, wrapper } = setup();
  client.setQueryData(keys.note("note-1"), { id: "note-1", title: "Runbook" });
  client.setQueryData(keys.note("note-2"), { id: "note-2", title: "Other" });
  route("DELETE /api/v1/notes/{id}", () => undefined);
  const { result } = renderHook(() => useDeleteNote(), { wrapper });

  await act(() => result.current.mutateAsync("note-1"));

  expect(client.getQueryData(keys.note("note-1"))).toBeUndefined();
  expect(invalidated(client, keys.note("note-2")), "other notes are left alone").toBe(false);
});

test("profile_polling_stops_once_a_profile_is_loaded", async () => {
  vi.useFakeTimers();
  const { client, wrapper } = setup();
  let collected = false;
  route("GET /api/v1/hosts/{name}/profile", () =>
    collected
      ? { collected_at: "2026-10-31T12:00:00Z", note: null, markdown: "", sections: [] }
      : failure(404, "not_found", "no profile yet"),
  );
  const asked = () => calls.filter((call) => call.route.endsWith("/profile")).length;
  const hook = renderHook(() => useProfile("nas"), { wrapper });
  const wait = (ms: number) => act(() => vi.advanceTimersByTimeAsync(ms));

  await wait(0);
  expect(asked()).toBe(1);
  await wait(PROFILE_POLL_MS);
  expect(asked(), "no profile yet, so it asks again").toBe(2);

  collected = true;
  await wait(PROFILE_POLL_MS);
  await wait(0);
  expect(asked()).toBe(3);
  expect(client.getQueryData(keys.hostProfile("nas"))).toBeDefined();

  await wait(PROFILE_POLL_MS * 5);
  expect(asked(), "it has one, so it stops asking").toBe(3);
  hook.unmount();
});
