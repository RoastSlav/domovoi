import { useState } from "react";
import { useEditSummaryField } from "../../api/hooks/experience";
import { ApiFailure, type SummaryField } from "../../api/types";
import { Button, Cluster, RelativeTime, Tag, Text, useActionFeedback } from "../../components";
import { actorLabel, summaryFieldLabel, summarySourceInfo } from "../../design/knowledgeText";
import { EditSummaryDialog } from "./EditSummaryDialog";
import { summaryValue } from "./summaryValue";
import styles from "./experience.module.css";

/**
 * One field of a resolution summary: its words, its value, the source of the text and who
 * wrote it. A field the owner may edit offers Edit, and Confirm while the text is still the
 * model's draft; Confirm sends the same text so it becomes the owner's. Code-owned fields have
 * no buttons.
 */
export function SummaryFieldRow({
  operationId,
  field,
  hostNames,
  me,
  onStale,
}: {
  field: SummaryField;
  hostNames: ReadonlyMap<string, string>;
  me: string | undefined;
  onStale: () => void;
  operationId: string;
}) {
  const [editing, setEditing] = useState(false);
  const confirm = useActionFeedback(useEditSummaryField(), {
    success: (data) =>
      data.unchanged
        ? { title: "Nothing changed: this already is your text.", tone: "info" }
        : "Confirmed. Domovoi now treats it as your words.",
    error: "confirm the text",
  });
  const source = summarySourceInfo(field.source);
  const text = summaryValue(field.field, field.value, hostNames);
  const confirmable = field.editable && field.source === "model" && typeof field.value === "string";
  return (
    <li className={styles.fieldRow}>
      <Text strong size="sm">
        {summaryFieldLabel(field.field)}
      </Text>
      <div className={styles.fieldBody}>
        <Text block>{text}</Text>
        <Cluster>
          <Tag hint={source.hint}>{source.label}</Tag>
          <Text muted size="sm">
            {actorLabel(field.actor, me)}, <RelativeTime at={field.created_at} />
          </Text>
          <Text muted size="sm">
            Revision {field.revision}
          </Text>
        </Cluster>
        {field.editable && (
          <Cluster>
            {confirmable && (
              <Button
                size="sm"
                loading={confirm.isPending}
                hint="Keeps this text and records it as written by you."
                onClick={() =>
                  confirm.mutate(
                    {
                      operation: operationId,
                      field: field.field,
                      value: text,
                      expectedRevision: field.revision,
                    },
                    {
                      onError: (failure) => {
                        if (failure instanceof ApiFailure && failure.status === 409) {
                          onStale();
                        }
                      },
                    },
                  )
                }
              >
                Confirm
              </Button>
            )}
            <Button size="sm" variant="secondary" onClick={() => setEditing(true)}>
              Edit
            </Button>
          </Cluster>
        )}
      </div>
      {editing && (
        <EditSummaryDialog
          operationId={operationId}
          fieldName={field.field}
          field={field}
          initial={typeof field.value === "string" ? field.value : text}
          onClose={() => setEditing(false)}
          onStale={onStale}
        />
      )}
    </li>
  );
}
