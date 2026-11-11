import { useCallback, useState } from "react";
import { describeError, type DescribedError } from "../design/messages";
import { useToast, type ToastInput } from "./Toast";

/** The part of a TanStack mutation the feedback wrapper drives. */
export interface FeedbackMutation<TData, TVariables> {
  isPending: boolean;
  mutateAsync: (variables: TVariables) => Promise<TData>;
  reset: () => void;
}

/** What a mutation says when it finishes. */
export interface FeedbackMessages<TData, TVariables> {
  /** The verb phrase for a failure: "acknowledge the incident". */
  error: string;
  /** `"inline"` raises no error toast: the caller shows `error` next to the control. */
  errorDisplay?: "toast" | "inline";
  /**
   * The success toast. A function may look at the result, and may return an `info` toast
   * for "nothing changed", or `null` to say nothing.
   */
  success:
    | string
    | ((data: TData, variables: TVariables) => string | (ToastInput & { tone?: "info" }) | null);
  /** Adds an Undo button to the success toast. */
  undo?: (data: TData, variables: TVariables) => void;
}

/** What to do after one call, on top of the messages. */
export interface FeedbackCallbacks<TData> {
  onError?: (error: unknown) => void;
  onSuccess?: (data: TData) => void;
}

/** A mutation that reports its own result. */
export interface ActionFeedback<TData, TVariables> {
  /** The last failure in words, for `errorDisplay: "inline"`; cleared by the next call. */
  error: DescribedError | undefined;
  isPending: boolean;
  /** Runs the mutation and reports how it went. Never rejects. */
  mutate: (variables: TVariables, callbacks?: FeedbackCallbacks<TData>) => void;
  /** Forgets the last failure. */
  reset: () => void;
}

/**
 * Wraps a mutation so its messages are declared in one place: a success toast (with Undo if
 * given), and on failure an error toast in the catalogue's words, or `error` for the caller
 * to show inline. The result is reported even if the component has gone away.
 *
 * Use it for every mutation. Keep errors inline (`errorDisplay: "inline"`, shown with
 * `InlineMessage`) when the control stays visible and the person must fix their input.
 */
export function useActionFeedback<TData, TVariables = void>(
  mutation: FeedbackMutation<TData, TVariables>,
  messages: FeedbackMessages<TData, TVariables>,
): ActionFeedback<TData, TVariables> {
  const toast = useToast();
  const [error, setError] = useState<DescribedError | undefined>(undefined);
  const { mutateAsync, reset: resetMutation } = mutation;

  const mutate = (variables: TVariables, callbacks?: FeedbackCallbacks<TData>): void => {
    setError(undefined);
    mutateAsync(variables).then(
      (data) => {
        const said =
          typeof messages.success === "function"
            ? messages.success(data, variables)
            : messages.success;
        if (said !== null) {
          const input: ToastInput = typeof said === "string" ? { title: said } : said;
          const undo = messages.undo;
          const withUndo: ToastInput = undo
            ? { ...input, action: { label: "Undo", onClick: () => undo(data, variables) } }
            : input;
          if (typeof said !== "string" && said.tone === "info") {
            toast.info(withUndo);
          } else {
            toast.success(withUndo);
          }
        }
        callbacks?.onSuccess?.(data);
      },
      (failure: unknown) => {
        const described = describeError(failure, messages.error);
        if (messages.errorDisplay === "inline") {
          setError(described);
        } else {
          toast.error(described);
        }
        callbacks?.onError?.(failure);
      },
    );
  };

  const reset = useCallback(() => {
    setError(undefined);
    resetMutation();
  }, [resetMutation]);

  return { mutate, isPending: mutation.isPending, error, reset };
}
