import { useMutation, useQueryClient } from "@tanstack/react-query";
import { api } from "../client";
import { keys } from "../queryClient";
import {
  call,
  defined,
  type PolicyDraftSaved,
  type PolicyPreview,
  type PolicyRule,
  type PolicySimulation,
  type PolicyVersionDetail,
} from "../types";

function useRefreshPolicy() {
  const client = useQueryClient();
  return () => client.invalidateQueries({ queryKey: keys.policy });
}

/** Starts the draft from a published version's rules, or empty when `from` is null. */
export function useStartDraft() {
  const onSuccess = useRefreshPolicy();
  return useMutation({
    mutationFn: async ({ from }: { from: number | null }) => {
      let rules: PolicyRule[] = [];
      if (from !== null) {
        const version = await call<PolicyVersionDetail>(
          api.GET("/api/v1/policy/versions/{number}", { params: { path: { number: from } } }),
        );
        rules = version.rules;
      }
      return call<PolicyDraftSaved>(
        api.PUT("/api/v1/policy/draft", { body: { base_revision: null, rules } }),
      );
    },
    onSuccess,
  });
}

/** What saving the draft sends; the password and hash only when a locked rule changes. */
export interface SaveDraftInput {
  baseRevision: number;
  rules: PolicyRule[];
  password?: string | undefined;
  previewHash?: string | undefined;
}

/** Saves the complete rule set as the draft. */
export function useSaveDraft() {
  const onSuccess = useRefreshPolicy();
  return useMutation({
    mutationFn: ({ baseRevision, rules, password, previewHash }: SaveDraftInput) =>
      call<PolicyDraftSaved>(
        api.PUT("/api/v1/policy/draft", {
          body: {
            base_revision: baseRevision,
            rules,
            ...defined({ password, preview_hash: previewHash }),
          },
        }),
      ),
    onSuccess,
  });
}

/** Discards the draft, naming the revision the client read. */
export function useDiscardDraft() {
  const onSuccess = useRefreshPolicy();
  return useMutation({
    mutationFn: ({ revision }: { revision: number }) =>
      call(api.DELETE("/api/v1/policy/draft", { params: { query: { revision } } })),
    onSuccess,
  });
}

/** Reviews a rule set against the active version; changes nothing. */
export function usePreviewRules() {
  return useMutation({
    mutationFn: ({ baseRevision, rules }: { baseRevision: number | null; rules: PolicyRule[] }) =>
      call<PolicyPreview>(
        api.POST("/api/v1/policy/preview", { body: { base_revision: baseRevision, rules } }),
      ),
  });
}

/** What publishing sends: exactly the preview's blocking keys and the password. */
export interface PublishInput {
  revision: number;
  previewHash: string;
  acknowledged: string[];
  password: string;
}

/** Publishes the draft as the next version. */
export function usePublishDraft() {
  const onSuccess = useRefreshPolicy();
  return useMutation({
    mutationFn: ({ revision, previewHash, acknowledged, password }: PublishInput) =>
      call(
        api.POST("/api/v1/policy/draft/publish", {
          body: { revision, preview_hash: previewHash, acknowledged, password },
        }),
      ),
    onSuccess,
  });
}

/** What a simulation sends. */
export interface SimulateInput {
  rules: { source: "draft" } | { source: "version"; number: number };
  selection: "enforced" | "shadow" | "both";
  inputs: "recorded" | "current";
  filter: {
    host?: string;
    contract?: string;
    status?: string;
    since?: string;
    until?: string;
  };
  execution_ids: string[];
  limit: number;
}

/** Re-runs recorded decisions under other rules; changes nothing. */
export function useSimulate() {
  return useMutation({
    mutationFn: (input: SimulateInput) =>
      call<PolicySimulation>(api.POST("/api/v1/policy/simulate", { body: input })),
  });
}
