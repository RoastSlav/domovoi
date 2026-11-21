import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useAcceptFact, useConfirmPath, useRetractFact } from "../../api/hooks/knowledgeWrites";
import { keys } from "../../api/queryClient";
import type { ManagementPath } from "../../api/types";
import { Button, ConfirmDialog, useActionFeedback } from "../../components";
import { needsPassword } from "../../design/knowledgeText";
import { FailureMessage } from "./FailureMessage";
import { useStepUp } from "./useStepUp";
import styles from "./paths.module.css";

/** Why Confirm cannot be used while the helper check is not available. */
export const CONFIRM_UNAVAILABLE =
  "Coverage cannot be verified yet, so no path can be confirmed. The helper check is not available in this version.";

const ACCEPT_REASON =
  "This kind of fact can affect protective actions. Enter your password to save it.";
const CONFIRM_REASON =
  "Confirming a management path tells Domovoi it is safe to rely on. Enter your password to confirm it.";

/** What an action on one path sends. */
interface Target {
  expectedVersion: number;
  id: string;
}

/**
 * The owner's buttons for one path, by its state: accept a path Domovoi found, confirm one the
 * owner entered, retract any. Confirm is disabled, with its reason and no request, while the
 * helper check is not available; accepting and confirming ask for the password.
 */
export function PathActions({
  path,
  coverageAvailable,
}: {
  coverageAvailable: boolean;
  path: ManagementPath;
}) {
  const client = useQueryClient();
  const refetch = () => void client.invalidateQueries({ queryKey: keys.knowledge });
  const target: Target = { id: path.fact.id, expectedVersion: path.fact.version };
  const [retracting, setRetracting] = useState(false);

  const accept = useActionFeedback(useAcceptFact(), {
    success: "Saved as your own path. Confirm it next.",
    error: "accept the path",
    errorDisplay: "inline",
  });
  const acceptStep = useStepUp<Target>({
    confirmLabel: "Accept",
    reason: () => ACCEPT_REASON,
    run: (variables, password, callbacks) =>
      accept.mutate(password === undefined ? variables : { ...variables, password }, {
        ...callbacks,
        onError: (failure) => {
          callbacks.onError?.(failure);
          refetch();
        },
      }),
    busy: accept.isPending,
    error: accept.error,
    reset: accept.reset,
  });

  const confirm = useActionFeedback(useConfirmPath(), {
    success: "Path confirmed.",
    error: "confirm the path",
    errorDisplay: "inline",
  });
  const confirmStep = useStepUp<Target>({
    confirmLabel: "Confirm",
    reason: () => CONFIRM_REASON,
    run: (variables, password, callbacks) =>
      confirm.mutate(
        { ...variables, password: password ?? "" },
        {
          ...callbacks,
          onError: (failure) => {
            callbacks.onError?.(failure);
            refetch();
          },
        },
      ),
    busy: confirm.isPending,
    error: confirm.error,
    reset: confirm.reset,
  });

  const retract = useActionFeedback(useRetractFact(), {
    success: "Path retracted.",
    error: "retract the path",
    errorDisplay: "inline",
  });
  const stopRetracting = () => {
    setRetracting(false);
    retract.reset();
  };

  const state = path.state;
  const canAccept = state === "candidate";
  const canConfirm = state === "unconfirmed" || state === "stale";

  return (
    <div className={styles.actions}>
      {canAccept && (
        <Button
          size="sm"
          variant="primary"
          hint="Saves it as your own path. The original stays as history."
          onClick={() => acceptStep.start(target, needsPassword("accept", "management_path"))}
        >
          Accept as mine
        </Button>
      )}
      {canConfirm && (
        <Button
          size="sm"
          variant={coverageAvailable ? "primary" : "secondary"}
          disabledReason={coverageAvailable ? undefined : CONFIRM_UNAVAILABLE}
          onClick={() => {
            if (coverageAvailable) {
              confirmStep.start(target, needsPassword("confirm", "management_path"));
            }
          }}
        >
          {state === "stale" ? "Confirm again" : "Confirm"}
        </Button>
      )}
      <Button size="sm" onClick={() => setRetracting(true)}>
        Retract
      </Button>
      <FailureMessage error={acceptStep.inline ?? confirmStep.inline} />
      {acceptStep.dialog}
      {confirmStep.dialog}
      {retracting && (
        <ConfirmDialog
          title="Retract this management path?"
          consequence="It stops being used and disappears from this list. Its content is removed after the retention period. You can add it again later."
          confirmLabel="Retract"
          tone="danger"
          busy={retract.isPending}
          error={retract.error}
          onCancel={stopRetracting}
          onConfirm={() => retract.mutate(target, { onSuccess: stopRetracting, onError: refetch })}
        />
      )}
    </div>
  );
}
