import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { api } from "../client";
import { keys } from "../queryClient";
import { call, defined, type OperationDetail, type OperationPage, type StepDetail } from "../types";

/** The operation list's filters, as kept in the URL. */
export interface OperationFilters {
  status?: string | undefined;
  incident?: string | undefined;
  host?: string | undefined;
  kind?: string | undefined;
}

/** The page size of the operation list. */
export const OPERATION_PAGE = 50;

/**
 * Operations matching `filters`, newest first, a page at a time.
 * Stays idle while `enabled` is false.
 */
export function useOperations(filters: OperationFilters, enabled = true) {
  return useInfiniteQuery({
    queryKey: keys.operationList(filters),
    enabled,
    queryFn: ({ pageParam }) =>
      call<OperationPage>(
        api.GET("/api/v1/operations", {
          params: { query: defined({ ...filters, cursor: pageParam, limit: OPERATION_PAGE }) },
        }),
      ),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.next_cursor ?? undefined,
  });
}

/** One operation with its trace and executions. */
export function useOperation(id: string) {
  return useQuery({
    queryKey: keys.operation(id),
    queryFn: () =>
      call<OperationDetail>(api.GET("/api/v1/operations/{id}", { params: { path: { id } } })),
  });
}

/** One step with its intent and bounded output; idle until a step is chosen. */
export function useOperationStep(operationId: string, stepId: string | undefined) {
  return useQuery({
    queryKey: keys.operationStep(operationId, stepId),
    enabled: stepId !== undefined,
    queryFn: () =>
      call<StepDetail>(
        api.GET("/api/v1/operations/{id}/steps/{step}", {
          params: { path: { id: operationId, step: stepId ?? "" } },
        }),
      ),
  });
}
