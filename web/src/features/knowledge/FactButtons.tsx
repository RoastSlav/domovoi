import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useNavigate } from "react-router";
import { useAcceptFact, useRetractFact, useVerifyFact } from "../../api/hooks/knowledgeWrites";
import { keys } from "../../api/queryClient";
import type { KnowledgeFact } from "../../api/types";
import { Button, ConfirmDialog, Cluster, useActionFeedback } from "../../components";
import { needsPassword, predicateInfo } from "../../design/knowledgeText";
import { factActions } from "./factActions";
import { FailureMessage } from "./FailureMessage";
import { subjectLabel, type SubjectNames } from "./subjects";
import { useStepUp, type StepUp } from "./useStepUp";

const SAFETY_REASON =
  "This kind of fact can affect protective actions. Enter your password to save it.";

/** What an action on a fact sends: the fact and the version the page last loaded. */
interface Target {
  expectedVersion: number;
  id: string;
  predicate: string;
}

/** Whether `fact` is the owner's to choose between two disputed values. */
export function isDisputed(fact: Pick<KnowledgeFact, "status">): boolean {
  return fact.status === "disputed";
}

/**
 * Accepts a fact (asking for the password when it is safety-critical) and opens the new
 * user-sourced row the hub made. A failed accept refetches, so the next try names the
 * current version.
 */
export function useAcceptAndOpen(): StepUp<Target> {
  const navigate = useNavigate();
  const client = useQueryClient();
  const accept = useActionFeedback(useAcceptFact(), {
    success: "Saved as your own fact. The original stays as history.",
    error: "accept the fact",
    errorDisplay: "inline",
  });
  return useStepUp<Target>({
    confirmLabel: "Accept",
    reason: () => SAFETY_REASON,
    run: (target, password, callbacks) =>
      accept.mutate(password === undefined ? target : { ...target, password }, {
        onSuccess: (fact) => {
          callbacks.onSuccess?.(fact);
          void navigate(`/knowledge/facts/${encodeURIComponent(fact.id)}`, { replace: true });
        },
        onError: (failure) => {
          callbacks.onError?.(failure);
          void client.invalidateQueries({ queryKey: keys.knowledge });
        },
      }),
    busy: accept.isPending,
    error: accept.error,
    reset: accept.reset,
  });
}

/**
 * The owner's buttons for one fact: accept ("Make it mine", or "Use this value" for a disputed
 * one), verify and retract. Which apply is `factActions`; the hub re-checks every one of them.
 */
export function FactActions({ fact, names }: { fact: KnowledgeFact; names: SubjectNames }) {
  const allowed = factActions(fact);
  const client = useQueryClient();
  const refetch = () => void client.invalidateQueries({ queryKey: keys.knowledge });
  const target: Target = { id: fact.id, expectedVersion: fact.version, predicate: fact.predicate };

  const accept = useAcceptAndOpen();

  const verify = useActionFeedback(useVerifyFact(), {
    success: "Marked as checked.",
    error: "verify the fact",
    errorDisplay: "inline",
  });
  const verifyStepUp = useStepUp<Target>({
    confirmLabel: "Verify",
    reason: () => SAFETY_REASON,
    run: (variables, password, callbacks) =>
      verify.mutate(password === undefined ? variables : { ...variables, password }, {
        ...callbacks,
        onError: (failure) => {
          callbacks.onError?.(failure);
          refetch();
        },
      }),
    busy: verify.isPending,
    error: verify.error,
    reset: verify.reset,
  });

  const retract = useActionFeedback(useRetractFact(), {
    success: "Retracted. It is no longer used.",
    error: "retract the fact",
    errorDisplay: "inline",
  });
  const [retracting, setRetracting] = useState(false);
  const stopRetracting = () => {
    setRetracting(false);
    retract.reset();
  };

  return (
    <>
      <Cluster gap={2}>
        {allowed.accept && (
          <Button
            variant="primary"
            hint="Saves it as your own fact. The original stays as history."
            onClick={() => accept.start(target, needsPassword("accept", fact.predicate))}
          >
            {isDisputed(fact) ? "Use this value" : "Make it mine"}
          </Button>
        )}
        {allowed.verify && (
          <Button
            hint="Marks it as checked today."
            onClick={() => verifyStepUp.start(target, needsPassword("verify", fact.predicate))}
          >
            Verify
          </Button>
        )}
        {allowed.retract && <Button onClick={() => setRetracting(true)}>Retract</Button>}
      </Cluster>
      <FailureMessage error={accept.inline ?? verifyStepUp.inline} />
      {accept.dialog}
      {verifyStepUp.dialog}
      {retracting && (
        <ConfirmDialog
          title={`Retract ${predicateInfo(fact.predicate).label} for ${subjectLabel(fact.subject, names).text}?`}
          consequence="It stops being used, disappears from lists and search, and is no longer shown to the model. Its content is removed after the retention period. You can enter it again later."
          confirmLabel="Retract"
          tone="danger"
          busy={retract.isPending}
          error={retract.error}
          onCancel={stopRetracting}
          onConfirm={() =>
            retract.mutate(
              { id: fact.id, expectedVersion: fact.version },
              { onSuccess: stopRetracting, onError: refetch },
            )
          }
        />
      )}
    </>
  );
}
