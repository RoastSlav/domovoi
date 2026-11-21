import { useMutation, useQueryClient } from "@tanstack/react-query";
import { api } from "../client";
import { keys } from "../queryClient";
import {
  call,
  type KnowledgeEntity,
  type KnowledgeFact,
  type KnowledgeRelation,
  type ManagementPath,
} from "../types";

/** Refreshes every knowledge list, detail, path, relation and entity after the owner's own change. */
function useRefresh() {
  const client = useQueryClient();
  return () => client.invalidateQueries({ queryKey: keys.knowledge });
}

/** What asserting a fact sends; the password only when the hub asks for one. */
export interface AssertFactVariables {
  subject: string;
  predicate: string;
  value: unknown;
  evidence?: string;
  validUntil?: string;
  password?: string | undefined;
}

/** The owner's own fact. The web never names a source or an actor: the hub records the signed-in owner. */
export function useAssertFact() {
  const refresh = useRefresh();
  return useMutation({
    retry: false,
    mutationFn: (v: AssertFactVariables) =>
      call<KnowledgeFact>(
        api.POST("/api/v1/knowledge/facts", {
          body: {
            subject: v.subject,
            predicate: v.predicate,
            value: v.value,
            ...(v.evidence === undefined ? {} : { evidence: v.evidence }),
            ...(v.validUntil === undefined ? {} : { valid_until: v.validUntil }),
            ...(v.password === undefined ? {} : { password: v.password }),
          },
        }),
      ),
    onSuccess: refresh,
  });
}

/** What an action on one fact sends. */
export interface FactActionVariables {
  id: string;
  expectedVersion: number;
  password?: string | undefined;
}

/** Makes an inferred, discovered or imported fact the owner's own; the original stays as history. */
export function useAcceptFact() {
  const refresh = useRefresh();
  return useMutation({
    retry: false,
    mutationFn: (v: FactActionVariables) =>
      call<KnowledgeFact>(
        api.POST("/api/v1/knowledge/facts/{id}/accept", {
          params: { path: { id: v.id } },
          body: {
            expected_version: v.expectedVersion,
            ...(v.password === undefined ? {} : { password: v.password }),
          },
        }),
      ),
    onSuccess: refresh,
  });
}

/** Withdraws a fact. It only removes trust, so it never takes a password. */
export function useRetractFact() {
  const refresh = useRefresh();
  return useMutation({
    retry: false,
    mutationFn: (v: { id: string; expectedVersion: number }) =>
      call<KnowledgeFact>(
        api.POST("/api/v1/knowledge/facts/{id}/retract", {
          params: { path: { id: v.id } },
          body: { expected_version: v.expectedVersion },
        }),
      ),
    onSuccess: refresh,
  });
}

/** Marks a fact as checked today. */
export function useVerifyFact() {
  const refresh = useRefresh();
  return useMutation({
    retry: false,
    mutationFn: (v: FactActionVariables) =>
      call<KnowledgeFact>(
        api.POST("/api/v1/knowledge/facts/{id}/verify", {
          params: { path: { id: v.id } },
          body: {
            expected_version: v.expectedVersion,
            ...(v.password === undefined ? {} : { password: v.password }),
          },
        }),
      ),
    onSuccess: refresh,
  });
}

/** Confirms a management path. The password is required by the type: every confirmation asks for it. */
export function useConfirmPath() {
  const refresh = useRefresh();
  return useMutation({
    retry: false,
    mutationFn: (v: { id: string; expectedVersion: number; password: string }) =>
      call<ManagementPath>(
        api.POST("/api/v1/knowledge/facts/{id}/confirm", {
          params: { path: { id: v.id } },
          body: { expected_version: v.expectedVersion, password: v.password },
        }),
      ),
    onSuccess: refresh,
  });
}

/** Links two subjects. */
export function useAssertRelation() {
  const refresh = useRefresh();
  return useMutation({
    retry: false,
    mutationFn: (v: { from: string; kind: string; to: string; evidence?: string }) =>
      call<KnowledgeRelation>(
        api.POST("/api/v1/knowledge/relations", {
          body: {
            from: v.from,
            kind: v.kind,
            to: v.to,
            ...(v.evidence === undefined ? {} : { evidence: v.evidence }),
          },
        }),
      ),
    onSuccess: refresh,
  });
}

/** Withdraws a relation. */
export function useRetractRelation() {
  const refresh = useRefresh();
  return useMutation({
    retry: false,
    mutationFn: (v: { id: string; expectedVersion: number }) =>
      call<KnowledgeRelation>(
        api.POST("/api/v1/knowledge/relations/{id}/retract", {
          params: { path: { id: v.id } },
          body: { expected_version: v.expectedVersion },
        }),
      ),
    onSuccess: refresh,
  });
}

/** Names a new thing facts can be about. */
export function useCreateEntity() {
  const refresh = useRefresh();
  return useMutation({
    retry: false,
    mutationFn: (v: { kind: string; name: string }) =>
      call<KnowledgeEntity>(
        api.POST("/api/v1/knowledge/entities", { body: { kind: v.kind, name: v.name } }),
      ),
    onSuccess: refresh,
  });
}

/** Renames an entity. */
export function useRenameEntity() {
  const refresh = useRefresh();
  return useMutation({
    retry: false,
    mutationFn: (v: { id: string; name: string; expectedVersion: number }) =>
      call<KnowledgeEntity>(
        api.PUT("/api/v1/knowledge/entities/{id}", {
          params: { path: { id: v.id } },
          body: { name: v.name, expected_version: v.expectedVersion },
        }),
      ),
    onSuccess: refresh,
  });
}

/** Deletes an entity no live fact or relation uses. */
export function useDeleteEntity() {
  const refresh = useRefresh();
  return useMutation({
    retry: false,
    mutationFn: async (v: { id: string; expectedVersion: number }) => {
      await call<unknown>(
        api.DELETE("/api/v1/knowledge/entities/{id}", {
          params: { path: { id: v.id }, query: { expected_version: v.expectedVersion } },
        }),
      );
    },
    onSuccess: refresh,
  });
}
