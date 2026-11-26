import { useState, type ReactNode } from "react";
import { ApiFailure } from "../../api/types";
import { PasswordPrompt } from "../../components";

/**
 * Runs a connection test or model lookup and asks for the password when the hub says the
 * candidate needs it (a CLI provider: the test runs that program). `run` is called without a
 * password first, and again with the one typed. Show `prompt` next to the button, and hide
 * the generic failure while `asking` is true.
 */
export function useCandidatePassword(
  run: (password?: string) => void,
  state: { error: unknown; isPending: boolean },
): { start: () => void; asking: boolean; prompt: ReactNode } {
  const [dismissed, setDismissed] = useState<unknown>(null);
  const [tried, setTried] = useState(false);
  const { error } = state;
  // The prompt shows while the newest failure is "needs the password" and was not cancelled;
  // a wrong password keeps it open with a message.
  // It stays open while the typed password is being tried.
  const asking =
    (error instanceof ApiFailure && error.status === 403 && dismissed !== error) ||
    (tried && state.isPending);
  const prompt = asking ? (
    <PasswordPrompt
      title="Confirm with your password"
      reason="Testing runs the command you entered, so it needs your password."
      confirmLabel="Run the test"
      busy={state.isPending}
      error={tried ? "That password is not right." : undefined}
      onConfirm={(password) => {
        setTried(true);
        run(password);
      }}
      onCancel={() => setDismissed(error)}
    />
  ) : null;
  return {
    start: () => {
      setTried(false);
      run();
    },
    asking,
    prompt,
  };
}
