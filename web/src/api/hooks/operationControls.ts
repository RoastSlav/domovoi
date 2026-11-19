import { useMutation, useQueryClient } from "@tanstack/react-query";
import { api } from "../client";
import { keys } from "../queryClient";
import { call, type BudgetExtension, type OperationControl } from "../types";

/** Refreshes every cache an operation command can change. */
function useRefreshOperations() {
  const client = useQueryClient();
  return async () => {
    await Promise.all([
      client.invalidateQueries({ queryKey: keys.operations }),
      client.invalidateQueries({ queryKey: keys.overview }),
      client.invalidateQueries({ queryKey: keys.incidentDetails }),
    ]);
  };
}

function useOperationChange<T>(change: (input: T) => Promise<unknown>) {
  return useMutation({ mutationFn: change, onSuccess: useRefreshOperations() });
}

/** What approving a step sends. */
export interface ApproveStepInput {
  step: string;
  /** The v2 plan hash the card showed; the hub refuses a different plan. */
  planHash: string;
  /** For a destructive or unknown-risk skill: its name, typed by the user. */
  confirmText?: string | undefined;
}

/** Approves an Execution step as the logged-in user. */
export function useApproveStep(operationId: string) {
  return useOperationChange((input: ApproveStepInput) =>
    call(
      api.POST("/api/v1/operations/{id}/steps/{step}/approve", {
        params: { path: { id: operationId, step: input.step } },
        body:
          input.confirmText === undefined
            ? { plan_hash: input.planHash }
            : { plan_hash: input.planHash, confirm_text: input.confirmText },
      }),
    ),
  );
}

/** Denies an Execution step as the logged-in user. */
export function useDenyStep(operationId: string) {
  return useOperationChange((input: { step: string }) =>
    call(
      api.POST("/api/v1/operations/{id}/steps/{step}/deny", {
        params: { path: { id: operationId, step: input.step } },
      }),
    ),
  );
}

/** Cancels an operation, with an optional reason kept in the history. */
export function useCancelOperation(id: string) {
  return useOperationChange((input: { reason?: string | undefined }) =>
    call<OperationControl>(
      api.POST("/api/v1/operations/{id}/cancel", {
        params: { path: { id } },
        body: input.reason === undefined ? {} : { reason: input.reason },
      }),
    ),
  );
}

/** Pauses an operation; nothing new starts until it is resumed. */
export function usePauseOperation(id: string) {
  return useMutation({
    mutationFn: () =>
      call<OperationControl>(
        api.POST("/api/v1/operations/{id}/pause", { params: { path: { id } } }),
      ),
    onSuccess: useRefreshOperations(),
  });
}

/** Resumes an operation, raising the given limits first. */
export function useResumeOperation(id: string) {
  return useOperationChange((input: { extend: BudgetExtension[] }) =>
    call<OperationControl>(
      api.POST("/api/v1/operations/{id}/resume", {
        params: { path: { id } },
        body: { extend: input.extend },
      }),
    ),
  );
}

/** Answers the question a step asked. */
export function useAnswerStep(id: string) {
  return useOperationChange((input: { step: string; text: string }) =>
    call<OperationControl>(
      api.POST("/api/v1/operations/{id}/steps/{step}/answer", {
        params: { path: { id, step: input.step } },
        body: { text: input.text },
      }),
    ),
  );
}
