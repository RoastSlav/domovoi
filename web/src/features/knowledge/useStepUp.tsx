import { useState, type ReactNode } from "react";
import { ApiFailure } from "../../api/types";
import { PasswordPrompt, type FeedbackCallbacks } from "../../components";
import type { DescribedError } from "../../design/messages";

/** What `useStepUp` hands to a control. */
export interface StepUp<V> {
  /** Runs the action now, or asks for the password first when `needsPassword`. */
  start(variables: V, needsPassword: boolean): void;
  /** The `PasswordPrompt` while one is open, else null. Render it once. */
  dialog: ReactNode;
  /** A failure to show next to the control when no prompt is open (a request that needed no password). */
  inline: DescribedError | undefined;
}

/** The sentence a wrong password gets, the same one the settings prompt uses. */
export const WRONG_PASSWORD = "That password is not right.";

/**
 * The password flow over `PasswordPrompt`: asks for the password only when the action needs
 * it, sends it once in the request and keeps it nowhere. A failure while the prompt is open
 * reads inside the prompt: the wrong-password sentence for a 403, the hub's words otherwise.
 */
export function useStepUp<V>(options: {
  confirmLabel: string;
  reason: (variables: V) => string;
  /** Calls the feedback mutation: `feedback.mutate(withPassword(variables, password), callbacks)`. */
  run: (variables: V, password: string | undefined, callbacks: FeedbackCallbacks<unknown>) => void;
  busy: boolean;
  /** The feedback mutation's `error` (it must use `errorDisplay: "inline"`). */
  error: DescribedError | undefined;
  reset: () => void;
}): StepUp<V> {
  const { confirmLabel, reason, run, busy, error, reset } = options;
  const [pending, setPending] = useState<{ variables: V } | undefined>(undefined);
  const [wrong, setWrong] = useState(false);

  const close = () => {
    setPending(undefined);
    setWrong(false);
  };

  const start = (variables: V, needsPassword: boolean) => {
    if (!needsPassword) {
      run(variables, undefined, {});
      return;
    }
    setWrong(false);
    setPending({ variables });
  };

  const words = error ? [error.title, error.detail].filter(Boolean).join(" ") : undefined;
  const dialog = pending ? (
    <PasswordPrompt
      reason={reason(pending.variables)}
      confirmLabel={confirmLabel}
      busy={busy}
      error={wrong ? WRONG_PASSWORD : words}
      onConfirm={(password) => {
        setWrong(false);
        run(pending.variables, password, {
          onSuccess: close,
          onError: (failure) => setWrong(failure instanceof ApiFailure && failure.status === 403),
        });
      }}
      onCancel={() => {
        close();
        reset();
      }}
    />
  ) : null;

  return { start, dialog, inline: pending ? undefined : error };
}
