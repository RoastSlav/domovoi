import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { createElement, type ReactNode } from "react";
import { calls, resetApi, route } from "../../test/apiMock";
import { FACTS_PAGE, knowledgeFact } from "../../test/knowledgeFixtures";
import { keys } from "../queryClient";
import { useFacts, useFactSearch, useRelations } from "./knowledge";
import {
  useAcceptFact,
  useAssertFact,
  useAssertRelation,
  useConfirmPath,
  useCreateEntity,
  useDeleteEntity,
  useRenameEntity,
  useRetractFact,
  useRetractRelation,
  useVerifyFact,
} from "./knowledgeWrites";

vi.mock("../client", () => import("../../test/apiMock"));

function setup() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const wrapper = ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client }, children);
  return { client, wrapper };
}

const body = (index: number) => calls[index]?.init.body;
const query = (index: number) => calls[index]?.init.params?.query;

beforeEach(resetApi);

describe("knowledge hooks", () => {
  it("fact_hooks_send_the_documented_requests", async () => {
    route("GET /api/v1/knowledge/facts", (init) =>
      init.params?.query?.cursor === undefined
        ? { ...FACTS_PAGE, next_cursor: "c2" }
        : { items: [], next_cursor: null },
    );
    const { wrapper } = setup();
    const list = renderHook(
      () =>
        useFacts({
          host: "nas",
          predicate: "role",
          source: "user",
          status: "stale",
          subject: "",
        }),
      { wrapper },
    );
    await waitFor(() => expect(list.result.current.isSuccess).toBe(true));
    await act(async () => {
      await list.result.current.fetchNextPage();
    });
    expect(query(0)).toEqual({
      host: "nas",
      predicate: "role",
      source: "user",
      status: "stale",
      limit: 50,
    });
    expect(query(1)).toEqual({ ...(query(0) as object), cursor: "c2" });

    const search = renderHook(() => useFactSearch({ q: " plex ", host: "nas" }), { wrapper });
    await waitFor(() => expect(search.result.current.isSuccess).toBe(true));
    expect(query(2)).toEqual({ q: "plex", host: "nas", limit: 50 });

    const before = calls.length;
    const idle = renderHook(() => useFactSearch({ q: "   " }), { wrapper });
    expect(idle.result.current.fetchStatus).toBe("idle");
    expect(calls).toHaveLength(before);
  });

  it("each_write_hook_sends_its_path_and_body", async () => {
    const fact = knowledgeFact();
    route("POST /api/v1/knowledge/facts", () => fact);
    route("POST /api/v1/knowledge/facts/{id}/accept", () => fact);
    route("POST /api/v1/knowledge/facts/{id}/retract", () => fact);
    route("POST /api/v1/knowledge/facts/{id}/verify", () => fact);
    route("POST /api/v1/knowledge/facts/{id}/confirm", () => ({}));
    route("POST /api/v1/knowledge/relations", () => ({}));
    route("POST /api/v1/knowledge/relations/{id}/retract", () => ({}));
    route("POST /api/v1/knowledge/entities", () => ({}));
    route("PUT /api/v1/knowledge/entities/{id}", () => ({}));
    route("DELETE /api/v1/knowledge/entities/{id}", () => undefined);
    const { wrapper } = setup();
    const assert = renderHook(() => useAssertFact(), { wrapper });
    const accept = renderHook(() => useAcceptFact(), { wrapper });
    const retract = renderHook(() => useRetractFact(), { wrapper });
    const verify = renderHook(() => useVerifyFact(), { wrapper });
    const confirm = renderHook(() => useConfirmPath(), { wrapper });
    const assertRelation = renderHook(() => useAssertRelation(), { wrapper });
    const retractRelation = renderHook(() => useRetractRelation(), { wrapper });
    const create = renderHook(() => useCreateEntity(), { wrapper });
    const rename = renderHook(() => useRenameEntity(), { wrapper });
    const remove = renderHook(() => useDeleteEntity(), { wrapper });
    await act(async () => {
      await assert.result.current.mutateAsync({
        subject: "host:host-nas",
        predicate: "role",
        value: "media server",
      });
      await assert.result.current.mutateAsync({
        subject: "host:host-nas",
        predicate: "trusted_cidr",
        value: { cidr: "10.0.0.0/8" },
        evidence: "note:n1",
        validUntil: "2027-01-01T00:00:00Z",
        password: "pw",
      });
      await accept.result.current.mutateAsync({ id: "f1", expectedVersion: 2 });
      await accept.result.current.mutateAsync({ id: "f1", expectedVersion: 2, password: "pw" });
      await retract.result.current.mutateAsync({ id: "f1", expectedVersion: 3 });
      await verify.result.current.mutateAsync({ id: "f1", expectedVersion: 4 });
      await verify.result.current.mutateAsync({ id: "f1", expectedVersion: 4, password: "pw" });
      await confirm.result.current.mutateAsync({ id: "f1", expectedVersion: 5, password: "pw" });
      await assertRelation.result.current.mutateAsync({
        from: "container:host-nas/plex",
        kind: "runs_on",
        to: "host:host-nas",
      });
      await retractRelation.result.current.mutateAsync({ id: "r1", expectedVersion: 1 });
      await create.result.current.mutateAsync({ kind: "vpn", name: "Home VPN" });
      await rename.result.current.mutateAsync({ id: "e1", name: "VPN", expectedVersion: 2 });
      await remove.result.current.mutateAsync({ id: "e1", expectedVersion: 3 });
    });
    expect(calls.map((c) => c.route)).toEqual([
      "POST /api/v1/knowledge/facts",
      "POST /api/v1/knowledge/facts",
      "POST /api/v1/knowledge/facts/{id}/accept",
      "POST /api/v1/knowledge/facts/{id}/accept",
      "POST /api/v1/knowledge/facts/{id}/retract",
      "POST /api/v1/knowledge/facts/{id}/verify",
      "POST /api/v1/knowledge/facts/{id}/verify",
      "POST /api/v1/knowledge/facts/{id}/confirm",
      "POST /api/v1/knowledge/relations",
      "POST /api/v1/knowledge/relations/{id}/retract",
      "POST /api/v1/knowledge/entities",
      "PUT /api/v1/knowledge/entities/{id}",
      "DELETE /api/v1/knowledge/entities/{id}",
    ]);
    expect(body(0)).toEqual({
      subject: "host:host-nas",
      predicate: "role",
      value: "media server",
    });
    expect(body(1)).toEqual({
      subject: "host:host-nas",
      predicate: "trusted_cidr",
      value: { cidr: "10.0.0.0/8" },
      evidence: "note:n1",
      valid_until: "2027-01-01T00:00:00Z",
      password: "pw",
    });
    expect(body(2)).toEqual({ expected_version: 2 });
    expect(body(3)).toEqual({ expected_version: 2, password: "pw" });
    expect(body(4)).toEqual({ expected_version: 3 });
    expect(body(5)).toEqual({ expected_version: 4 });
    expect(body(6)).toEqual({ expected_version: 4, password: "pw" });
    expect(body(7)).toEqual({ expected_version: 5, password: "pw" });
    expect(body(8)).toEqual({
      from: "container:host-nas/plex",
      kind: "runs_on",
      to: "host:host-nas",
    });
    expect(body(9)).toEqual({ expected_version: 1 });
    expect(body(10)).toEqual({ kind: "vpn", name: "Home VPN" });
    expect(body(11)).toEqual({ name: "VPN", expected_version: 2 });
    expect(body(12)).toBeUndefined();
    expect(query(12)).toEqual({ expected_version: 3 });
    expect(calls[2]?.init.params?.path).toEqual({ id: "f1" });
    // The web never names a source or an actor, and a retract never carries a password.
    for (const call of calls) {
      const sent = call.init.body as object | undefined;
      expect(sent === undefined || !("source" in sent)).toBe(true);
      expect(sent === undefined || !("actor" in sent)).toBe(true);
    }
    for (const index of [4, 9]) {
      expect(body(index)).not.toHaveProperty("password");
    }
    expect(body(7)).toHaveProperty("password");
  });

  it("each_write_invalidates_the_knowledge_prefix_and_nothing_else", async () => {
    route("POST /api/v1/knowledge/facts", () => knowledgeFact());
    route("POST /api/v1/knowledge/facts/{id}/retract", () => knowledgeFact());
    route("DELETE /api/v1/knowledge/entities/{id}", () => undefined);
    const { client, wrapper } = setup();
    const invalidate = vi.spyOn(client, "invalidateQueries");
    const setData = vi.spyOn(client, "setQueryData");
    const assert = renderHook(() => useAssertFact(), { wrapper });
    const retract = renderHook(() => useRetractFact(), { wrapper });
    const remove = renderHook(() => useDeleteEntity(), { wrapper });
    await act(async () => {
      await assert.result.current.mutateAsync({
        subject: "host:host-nas",
        predicate: "role",
        value: "x",
      });
      await retract.result.current.mutateAsync({ id: "f1", expectedVersion: 1 });
      await remove.result.current.mutateAsync({ id: "e1", expectedVersion: 1 });
    });
    expect(invalidate.mock.calls).toEqual([
      [{ queryKey: keys.knowledge }],
      [{ queryKey: keys.knowledge }],
      [{ queryKey: keys.knowledge }],
    ]);
    expect(setData).not.toHaveBeenCalled();

    // No retry: a failed write is attempted once.
    resetApi();
    let attempts = 0;
    route("POST /api/v1/knowledge/facts", () => {
      attempts += 1;
      throw new Error("boom");
    });
    await act(async () => {
      await assert.result.current
        .mutateAsync({ subject: "host:host-nas", predicate: "role", value: "x" })
        .catch(() => undefined);
    });
    expect(attempts).toBe(1);
  });

  it("relations_hook_sends_include_retracted_only_when_true", async () => {
    route("GET /api/v1/knowledge/relations", () => ({ items: [], next_cursor: null }));
    const { wrapper } = setup();
    const plain = renderHook(() => useRelations({ kind: "runs_on", include_retracted: false }), {
      wrapper,
    });
    await waitFor(() => expect(plain.result.current.isSuccess).toBe(true));
    const retracted = renderHook(() => useRelations({ include_retracted: true }), { wrapper });
    await waitFor(() => expect(retracted.result.current.isSuccess).toBe(true));
    expect(query(0)).toEqual({ kind: "runs_on", limit: 50 });
    expect(query(1)).toEqual({ include_retracted: true, limit: 50 });
  });
});
