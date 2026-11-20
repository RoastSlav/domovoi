import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "../client";
import { keys } from "../queryClient";
import {
  call,
  defined,
  type EditSummaryResponse,
  type ExperienceSearch,
  type ResolutionSummary,
  type SimilarOperations,
} from "../types";

/** The summary of an operation, with where each field came from; idle without an id. */
export function useResolutionSummary(operationId: string | undefined) {
  return useQuery({
    queryKey: keys.operationSummary(operationId ?? ""),
    enabled: operationId !== undefined,
    queryFn: () =>
      call<ResolutionSummary>(
        api.GET("/api/v1/operations/{id}/summary", {
          params: { path: { id: operationId ?? "" } },
        }),
      ),
  });
}

/** What the similar-operations list is about: an operation or an incident. */
export type SimilarSource = { operation: string } | { incident: string };

/** Past operations that resemble an operation or an incident; idle without a source. */
export function useSimilarOperations(source: SimilarSource | undefined, limit?: number) {
  const query = defined({ limit });
  return useQuery({
    queryKey:
      source === undefined
        ? keys.operationSimilar("", limit)
        : "operation" in source
          ? keys.operationSimilar(source.operation, limit)
          : keys.incidentSimilar(source.incident, limit),
    enabled: source !== undefined,
    queryFn: () => {
      if (source !== undefined && "incident" in source) {
        return call<SimilarOperations>(
          api.GET("/api/v1/incidents/{id}/similar-operations", {
            params: { path: { id: source.incident }, query },
          }),
        );
      }
      return call<SimilarOperations>(
        api.GET("/api/v1/operations/{id}/similar", {
          params: { path: { id: source?.operation ?? "" }, query },
        }),
      );
    },
  });
}

/** Where to search: one host by name, or every host when the owner asks for it. */
export type ExperienceSearchParams =
  { q: string; scope: "host"; host: string } | { q: string; scope: "global" };

/** The owner's search over past operations; idle without parameters. */
export function useExperienceSearch(params: ExperienceSearchParams | undefined) {
  return useQuery({
    queryKey: keys.experienceSearch(params ?? {}),
    enabled: params !== undefined,
    queryFn: () =>
      call<ExperienceSearch>(
        api.GET("/api/v1/experience/search", {
          params: { query: defined({ ...params }) },
        }),
      ),
  });
}

/** What an edit of a narrative field sends. */
export interface EditSummaryFieldInput {
  operation: string;
  /** `problem`, `cause` or `fix`. */
  field: string;
  value: string;
  /** The field's newest revision when it was opened; 0 when it had none. */
  expectedRevision: number;
}

/**
 * Writes or confirms a narrative field as the logged-in user. Does not retry: a stale edit
 * needs the person to look again. Refreshes everything under the operation.
 */
export function useEditSummaryField() {
  const client = useQueryClient();
  return useMutation({
    retry: false,
    mutationFn: (input: EditSummaryFieldInput) =>
      call<EditSummaryResponse>(
        api.POST("/api/v1/operations/{id}/summary/{field}", {
          params: { path: { id: input.operation, field: input.field } },
          body: { value: input.value, expected_revision: input.expectedRevision },
        }),
      ),
    onSuccess: (_data, input) =>
      client.invalidateQueries({ queryKey: keys.operation(input.operation) }),
  });
}
