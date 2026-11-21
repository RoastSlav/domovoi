import { useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { useRetractRelation } from "../../api/hooks/knowledgeWrites";
import { keys } from "../../api/queryClient";
import type { KnowledgeRelation } from "../../api/types";
import {
  Button,
  ConfirmDialog,
  DataTable,
  RelativeTime,
  StatusPill,
  Tag,
  useActionFeedback,
  type Column,
} from "../../components";
import { relationPhrase, sourceInfo } from "../../design/knowledgeText";
import { knowledgeStatusMeta } from "../../design/tone";
import { REMOVED_VALUE } from "./FactsTable";
import { RelationDialog, relationIsLive } from "./RelationDialog";
import { subjectLabel, type SubjectNames } from "./subjects";

/** A relation in words: "plex runs on nas". */
export function relationText(relation: KnowledgeRelation, names: SubjectNames): string {
  if (relation.content_purged || !relation.from || !relation.to) {
    return REMOVED_VALUE;
  }
  const from = subjectLabel(relation.from, names).text;
  const to = subjectLabel(relation.to, names).text;
  return `${from} ${relationPhrase(relation.kind)} ${to}`;
}

/** Relations as a table; a row opens its detail and a live one can be retracted. */
export function RelationsTable({
  relations,
  names,
  caption,
}: {
  caption: string;
  names: SubjectNames;
  relations: readonly KnowledgeRelation[];
}) {
  const [open, setOpen] = useState<KnowledgeRelation | undefined>(undefined);
  const [retracting, setRetracting] = useState<KnowledgeRelation | undefined>(undefined);
  const client = useQueryClient();
  const retract = useRetractRelation();
  const feedback = useActionFeedback(retract, {
    success: "Retracted. It is no longer used.",
    error: "retract the relation",
    errorDisplay: "inline",
  });

  const columns = useMemo<Column<KnowledgeRelation>[]>(
    () => [
      {
        key: "relation",
        label: "Relation",
        cell: (relation) => (
          <Button variant="ghost" onClick={() => setOpen(relation)}>
            {relationText(relation, names)}
          </Button>
        ),
      },
      {
        key: "source",
        label: "Source",
        cell: (r) => <Tag hint={sourceInfo(r.source).hint}>{sourceInfo(r.source).label}</Tag>,
      },
      {
        key: "status",
        label: "Status",
        cell: (r) => <StatusPill size="sm" meta={knowledgeStatusMeta(r.status)} />,
      },
      { key: "updated", label: "Updated", cell: (r) => <RelativeTime at={r.observed_at} /> },
      {
        key: "actions",
        label: "Actions",
        hideLabel: true,
        cell: (relation) =>
          relationIsLive(relation) ? (
            <Button size="sm" onClick={() => setRetracting(relation)}>
              Retract
            </Button>
          ) : null,
      },
    ],
    [names],
  );

  const stopRetracting = () => {
    setRetracting(undefined);
    feedback.reset();
  };
  const confirm = () => {
    if (!retracting) {
      return;
    }
    feedback.mutate(
      { id: retracting.id, expectedVersion: retracting.version },
      {
        onSuccess: stopRetracting,
        onError: () => void client.invalidateQueries({ queryKey: keys.knowledge }),
      },
    );
  };

  return (
    <>
      <DataTable
        caption={caption}
        columns={columns}
        rows={relations}
        rowKey={(relation) => relation.id}
      />
      {open && (
        <RelationDialog
          relation={open}
          names={names}
          onClose={() => setOpen(undefined)}
          onRetract={(relation) => {
            setOpen(undefined);
            setRetracting(relation);
          }}
        />
      )}
      {retracting && (
        <ConfirmDialog
          title={`Retract “${relationText(retracting, names)}”?`}
          consequence="It stops being used and disappears from lists and search. Its content is removed after the retention period. You can enter it again later."
          confirmLabel="Retract"
          tone="danger"
          busy={feedback.isPending}
          error={feedback.error}
          onConfirm={confirm}
          onCancel={stopRetracting}
        />
      )}
    </>
  );
}
