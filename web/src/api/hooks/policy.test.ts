import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { createElement, type ReactNode } from "react";
import { calls, resetApi, route } from "../../test/apiMock";
import {
  DRAFT_4,
  OVERVIEW_ACTIVE,
  PREVIEW_CLEAN,
  SHADOW_PAGE_ONE,
  VERSION_3,
} from "../../test/policyFixtures";
import { keys } from "../queryClient";
import {
  SHADOW_PAGE,
  useDraftPreview,
  usePolicyDraft,
  usePolicyOverview,
  usePolicyVersion,
  usePolicyVersions,
  useShadowDecision,
  useShadowDecisions,
} from "./policy";
import {
  useDiscardDraft,
  usePreviewRules,
  usePublishDraft,
  useSaveDraft,
  useSimulate,
  useStartDraft,
  type SimulateInput,
} from "./policyCommands";

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

describe("policy hooks", () => {
  it("overview_version_and_draft_hooks_call_their_routes", async () => {
    route("GET /api/v1/policy", () => OVERVIEW_ACTIVE);
    route("GET /api/v1/policy/versions/{number}", () => VERSION_3);
    route("GET /api/v1/policy/draft", () => DRAFT_4);
    route("GET /api/v1/policy/shadow/{execution_id}", () => ({}));
    const { wrapper } = setup();
    const overview = renderHook(() => usePolicyOverview(), { wrapper });
    const version = renderHook(() => usePolicyVersion(3), { wrapper });
    const draft = renderHook(() => usePolicyDraft(true), { wrapper });
    const decision = renderHook(() => useShadowDecision("exec-1"), { wrapper });
    await waitFor(() => expect(overview.result.current.isSuccess).toBe(true));
    await waitFor(() => expect(version.result.current.isSuccess).toBe(true));
    await waitFor(() => expect(draft.result.current.isSuccess).toBe(true));
    await waitFor(() => expect(decision.result.current.isSuccess).toBe(true));
    const paths = Object.fromEntries(calls.map((c) => [c.route, c.init.params?.path]));
    expect(paths["GET /api/v1/policy/versions/{number}"]).toEqual({ number: 3 });
    expect(paths["GET /api/v1/policy/shadow/{execution_id}"]).toEqual({ execution_id: "exec-1" });
    const idle = renderHook(() => usePolicyVersion(undefined), { wrapper });
    const idleDraft = renderHook(() => usePolicyDraft(false), { wrapper });
    expect(idle.result.current.fetchStatus).toBe("idle");
    expect(idleDraft.result.current.fetchStatus).toBe("idle");
    expect(calls).toHaveLength(4);
  });

  it("versions_and_decisions_page_with_the_cursor_and_drop_empty_filters", async () => {
    route("GET /api/v1/policy/versions", (init) =>
      init.params?.query?.cursor === undefined
        ? { items: [], next_cursor: "v2" }
        : { items: [], next_cursor: null },
    );
    route("GET /api/v1/policy/shadow", (init) =>
      init.params?.query?.cursor === undefined ? SHADOW_PAGE_ONE : { items: [] },
    );
    const { wrapper } = setup();
    const versions = renderHook(() => usePolicyVersions(), { wrapper });
    await waitFor(() => expect(versions.result.current.isSuccess).toBe(true));
    await act(async () => {
      await versions.result.current.fetchNextPage();
    });
    const shadow = renderHook(
      () => useShadowDecisions({ host: "h1", contract: "", status: undefined, differing: false }),
      { wrapper },
    );
    await waitFor(() => expect(shadow.result.current.isSuccess).toBe(true));
    await act(async () => {
      await shadow.result.current.fetchNextPage();
    });
    expect(query(0)).toEqual({ limit: 25 });
    expect(query(1)).toEqual({ limit: 25, cursor: "v2" });
    expect(query(2)).toEqual({ host: "h1", limit: SHADOW_PAGE });
    expect(query(3)).toEqual({
      host: "h1",
      limit: SHADOW_PAGE,
      cursor: SHADOW_PAGE_ONE.next_cursor,
    });
    const differing = renderHook(() => useShadowDecisions({ differing: true }), { wrapper });
    await waitFor(() => expect(differing.result.current.isSuccess).toBe(true));
    expect(query(4)).toEqual({ differing: true, limit: SHADOW_PAGE });
  });

  it("start_draft_copies_a_versions_rules_or_starts_empty", async () => {
    route("GET /api/v1/policy/versions/{number}", () => VERSION_3);
    route("PUT /api/v1/policy/draft", () => ({ draft: DRAFT_4, locked_changes: [] }));
    const { wrapper } = setup();
    const { result } = renderHook(() => useStartDraft(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({ from: 3 });
      await result.current.mutateAsync({ from: null });
    });
    expect(calls.map((c) => c.route)).toEqual([
      "GET /api/v1/policy/versions/{number}",
      "PUT /api/v1/policy/draft",
      "PUT /api/v1/policy/draft",
    ]);
    expect(body(1)).toEqual({ base_revision: null, rules: VERSION_3.rules });
    expect(body(2)).toEqual({ base_revision: null, rules: [] });
  });

  it("save_draft_omits_password_and_hash_unless_given", async () => {
    route("PUT /api/v1/policy/draft", () => ({ draft: DRAFT_4, locked_changes: [] }));
    const { wrapper } = setup();
    const { result } = renderHook(() => useSaveDraft(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({ baseRevision: 4, rules: [] });
      await result.current.mutateAsync({
        baseRevision: 4,
        rules: [],
        password: "pw",
        previewHash: "h",
      });
    });
    expect(body(0)).toEqual({ base_revision: 4, rules: [] });
    expect(body(1)).toEqual({ base_revision: 4, rules: [], password: "pw", preview_hash: "h" });
  });

  it("discard_sends_the_revision_in_the_query", async () => {
    route("DELETE /api/v1/policy/draft", () => undefined);
    const { wrapper } = setup();
    const { result } = renderHook(() => useDiscardDraft(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({ revision: 4 });
    });
    expect(query(0)).toEqual({ revision: 4 });
  });

  it("preview_publish_and_simulate_send_the_documented_bodies", async () => {
    route("POST /api/v1/policy/preview", () => PREVIEW_CLEAN);
    route("POST /api/v1/policy/draft/publish", () => ({}));
    route("POST /api/v1/policy/simulate", () => ({}));
    const { wrapper } = setup();
    const preview = renderHook(() => usePreviewRules(), { wrapper });
    const publish = renderHook(() => usePublishDraft(), { wrapper });
    const simulate = renderHook(() => useSimulate(), { wrapper });
    const input: SimulateInput = {
      rules: { source: "version", number: 3 },
      selection: "both",
      inputs: "recorded",
      filter: { host: "h1" },
      execution_ids: ["e1"],
      limit: 200,
    };
    await act(async () => {
      await preview.result.current.mutateAsync({ baseRevision: 4, rules: [] });
      await publish.result.current.mutateAsync({
        revision: 4,
        previewHash: "h",
        acknowledged: ["k1"],
        password: "pw",
      });
      await simulate.result.current.mutateAsync(input);
    });
    expect(body(0)).toEqual({ base_revision: 4, rules: [] });
    expect(body(1)).toEqual({
      revision: 4,
      preview_hash: "h",
      acknowledged: ["k1"],
      password: "pw",
    });
    expect(body(2)).toEqual(input);
  });

  it("commands_that_change_policy_invalidate_every_policy_query_and_reads_do_not", async () => {
    route("PUT /api/v1/policy/draft", () => ({ draft: DRAFT_4, locked_changes: [] }));
    route("DELETE /api/v1/policy/draft", () => undefined);
    route("POST /api/v1/policy/draft/publish", () => ({}));
    route("POST /api/v1/policy/preview", () => PREVIEW_CLEAN);
    route("POST /api/v1/policy/simulate", () => ({}));
    const { client, wrapper } = setup();
    const seed = () => {
      client.clear();
      client.setQueryData(keys.policyOverview, OVERVIEW_ACTIVE);
      client.setQueryData(keys.policyVersion(3), VERSION_3);
      client.setQueryData(keys.policyShadow({}), SHADOW_PAGE_ONE);
    };
    const stale = () =>
      [keys.policyOverview, keys.policyVersion(3), keys.policyShadow({})].map(
        (key) => client.getQueryState(key)?.isInvalidated,
      );
    const save = renderHook(() => useSaveDraft(), { wrapper });
    const start = renderHook(() => useStartDraft(), { wrapper });
    const discard = renderHook(() => useDiscardDraft(), { wrapper });
    const publish = renderHook(() => usePublishDraft(), { wrapper });
    const preview = renderHook(() => usePreviewRules(), { wrapper });
    const simulate = renderHook(() => useSimulate(), { wrapper });
    const runs: [string, () => Promise<unknown>][] = [
      ["save", () => save.result.current.mutateAsync({ baseRevision: 4, rules: [] })],
      ["start", () => start.result.current.mutateAsync({ from: null })],
      ["discard", () => discard.result.current.mutateAsync({ revision: 4 })],
      [
        "publish",
        () =>
          publish.result.current.mutateAsync({
            revision: 4,
            previewHash: "h",
            acknowledged: [],
            password: "pw",
          }),
      ],
    ];
    for (const [name, run] of runs) {
      seed();
      await act(async () => {
        await run();
      });
      expect(stale(), name).toEqual([true, true, true]);
    }
    seed();
    await act(async () => {
      await preview.result.current.mutateAsync({ baseRevision: 4, rules: [] });
      await simulate.result.current.mutateAsync({
        rules: { source: "draft" },
        selection: "both",
        inputs: "current",
        filter: {},
        execution_ids: [],
        limit: 10,
      });
    });
    expect(stale()).toEqual([false, false, false]);
  });

  it("the_draft_preview_query_is_keyed_by_revision_and_fingerprint", async () => {
    route("POST /api/v1/policy/preview", () => PREVIEW_CLEAN);
    const { client, wrapper } = setup();
    const { result } = renderHook(() => useDraftPreview(DRAFT_4), { wrapper });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(body(0)).toEqual({ base_revision: DRAFT_4.revision, rules: DRAFT_4.rules });
    expect(client.getQueryData(keys.policyPreview(DRAFT_4.revision, DRAFT_4.rules_hash))).toEqual(
      PREVIEW_CLEAN,
    );
    const idle = renderHook(() => useDraftPreview(undefined), { wrapper });
    expect(idle.result.current.fetchStatus).toBe("idle");
    expect(calls).toHaveLength(1);
  });
});
