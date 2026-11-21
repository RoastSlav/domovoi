import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useFacts } from "../../api/hooks/knowledge";
import { useAcceptFact, useRetractFact } from "../../api/hooks/knowledgeWrites";
import { keys } from "../../api/queryClient";
import type { KnowledgeFact } from "../../api/types";
import { useSelectedHost } from "../../app/HostContext";
import {
  Button,
  Card,
  ConfirmDialog,
  EmptyState,
  QueryBoundary,
  RelativeTime,
  Skeleton,
  StatusPill,
  Text,
  Visible,
  useActionFeedback,
} from "../../components";
import { needsPassword, predicateInfo } from "../../design/knowledgeText";
import { knowledgeStatusMeta } from "../../design/tone";
import { EvidenceRef } from "./EvidenceRef";
import { FailureMessage } from "./FailureMessage";
import { factValueText } from "./FactsTable";
import { SubjectLink } from "./SubjectLink";
import { subjectLabel } from "./subjects";
import { useStepUp } from "./useStepUp";
import { useSubjectLabels } from "./useSubjectLabels";
import styles from "./knowledge.module.css";

const ACCEPT_REASON =
  "This kind of fact can affect protective actions. Enter your password to save it.";

/** What accepting a proposal sends. */
interface Accepting {
  expectedVersion: number;
  id: string;
  predicate: string;
}

/**
 * The facts Domovoi inferred, which wait for the owner's word: accept saves one as the owner's
 * own (the original stays as history), retract withdraws it.
 */
export function ProposalsTab() {
  const [host] = useSelectedHost();
  const names = useSubjectLabels();
  const client = useQueryClient();
  const proposals = useFacts({ source: "inferred", host });
  const [retracting, setRetracting] = useState<KnowledgeFact | undefined>(undefined);
  const refetch = () => void client.invalidateQueries({ queryKey: keys.knowledge });

  const accept = useActionFeedback(useAcceptFact(), {
    success: "Saved as your own fact. The original stays as history.",
    error: "accept the fact",
    errorDisplay: "inline",
  });
  const stepUp = useStepUp<Accepting>({
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

  const retract = useActionFeedback(useRetractFact(), {
    success: "Retracted. It is no longer used.",
    error: "retract the fact",
    errorDisplay: "inline",
  });
  const stopRetracting = () => {
    setRetracting(undefined);
    retract.reset();
  };

  return (
    <Visible category="P0" id="knowledge.proposals">
      <FailureMessage error={stepUp.inline} />
      <QueryBoundary
        query={proposals}
        context="load proposals"
        loading={<Skeleton shape="rows" count={3} />}
        isEmpty={(data) => data.pages.every((page) => page.items.length === 0)}
        empty={
          <EmptyState icon="check-circle" title="Nothing waits for your word">
            <p>When Domovoi infers something, it appears here until you accept or retract it.</p>
          </EmptyState>
        }
      >
        {(data) => (
          <>
            <div className={styles.stack}>
              {data.pages
                .flatMap((page) => page.items)
                .map((fact) => (
                  <Card key={fact.id} label={predicateInfo(fact.predicate).label}>
                    <div className={styles.proposal}>
                      <div className={styles.proposalHead}>
                        <span>
                          <Text strong>{predicateInfo(fact.predicate).label}</Text>{" "}
                          <Text muted>
                            <SubjectLink subject={fact.subject} names={names} />
                          </Text>
                        </span>
                        <StatusPill size="sm" meta={knowledgeStatusMeta(fact.status)} />
                      </div>
                      <Text block>
                        <span className={styles.value}>{factValueText(fact, names)}</span>
                      </Text>
                      <Text muted block>
                        Inferred by Domovoi <RelativeTime at={fact.observed_at} />
                        {fact.evidence ? (
                          <>
                            {" · Evidence: "}
                            <EvidenceRef evidence={fact.evidence} />
                          </>
                        ) : null}
                      </Text>
                      <div className={styles.actions}>
                        <Button
                          variant="primary"
                          hint="Saves it as your own fact. The original stays as history."
                          onClick={() =>
                            stepUp.start(
                              {
                                id: fact.id,
                                expectedVersion: fact.version,
                                predicate: fact.predicate,
                              },
                              needsPassword("accept", fact.predicate),
                            )
                          }
                        >
                          Accept
                        </Button>
                        <Button onClick={() => setRetracting(fact)}>Retract</Button>
                      </div>
                    </div>
                  </Card>
                ))}
            </div>
            {proposals.hasNextPage && (
              <div className={styles.more}>
                <Button
                  loading={proposals.isFetchingNextPage}
                  onClick={() => void proposals.fetchNextPage()}
                >
                  Load more
                </Button>
              </div>
            )}
          </>
        )}
      </QueryBoundary>
      {stepUp.dialog}
      {retracting && (
        <ConfirmDialog
          title={`Retract ${predicateInfo(retracting.predicate).label} for ${subjectLabel(retracting.subject, names).text}?`}
          consequence="It stops being used, disappears from lists and search, and is no longer shown to the model. Its content is removed after the retention period. You can enter it again later."
          confirmLabel="Retract"
          tone="danger"
          busy={retract.isPending}
          error={retract.error}
          onCancel={stopRetracting}
          onConfirm={() =>
            retract.mutate(
              { id: retracting.id, expectedVersion: retracting.version },
              {
                onSuccess: stopRetracting,
                onError: refetch,
              },
            )
          }
        />
      )}
    </Visible>
  );
}
