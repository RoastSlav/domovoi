import { useCallback, useEffect, useRef, type ReactNode } from "react";
import { useBlocker } from "react-router";
import { ConfirmDialog } from "../../components";

/**
 * Holds the person on the page while `dirty`: a "Leave without saving?" question for
 * in-app navigation, the Back button included (render `guard` somewhere in the page), and
 * the browser's own prompt for closing or reloading the tab. Call `allowLeaving()` just
 * before navigating away on purpose, after a save or a delete.
 */
export function useLeaveGuard(dirty: boolean): {
  allowLeaving: () => void;
  guard: ReactNode;
} {
  const allowedRef = useRef(false);
  const blocker = useBlocker(() => dirty && !allowedRef.current);

  useEffect(() => {
    if (!dirty) {
      return;
    }
    const onBeforeUnload = (event: BeforeUnloadEvent) => {
      if (!allowedRef.current) {
        event.preventDefault();
      }
    };
    window.addEventListener("beforeunload", onBeforeUnload);
    return () => window.removeEventListener("beforeunload", onBeforeUnload);
  }, [dirty]);

  const allowLeaving = useCallback(() => {
    allowedRef.current = true;
  }, []);
  const guard =
    blocker.state === "blocked" ? (
      <ConfirmDialog
        title="Leave without saving?"
        consequence="Your changes to this note are lost if you leave this page."
        confirmLabel="Leave without saving"
        tone="danger"
        onConfirm={() => blocker.proceed()}
        onCancel={() => blocker.reset()}
      />
    ) : null;
  return { allowLeaving, guard };
}
