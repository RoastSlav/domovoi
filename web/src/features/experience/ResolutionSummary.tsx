import { useResolutionSummary } from "../../api/hooks/experience";
import { useHostNames } from "../../api/hooks/hosts";
import { useSession } from "../../api/hooks/session";
import type { ResolutionSummary as Summary } from "../../api/types";
import {
  Button,
  Progress,
  QueryBoundary,
  Section,
  Skeleton,
  Stack,
  Visible,
} from "../../components";
import { NARRATIVE_FIELDS, summaryFieldLabel } from "../../design/knowledgeText";
import { EditSummaryDialog } from "./EditSummaryDialog";
import { SummaryFieldRow } from "./SummaryFieldRow";
import { SummaryHistory } from "./SummaryHistory";
import { useState } from "react";
import styles from "./experience.module.css";

const TITLE = "Resolution summary";

/** A narrative field the summary has no revision of yet, with a button to write it. */
function MissingField({
  operationId,
  fieldName,
  onStale,
}: {
  fieldName: string;
  onStale: () => void;
  operationId: string;
}) {
  const [adding, setAdding] = useState(false);
  return (
    <li className={styles.fieldRow}>
      <Button variant="ghost" size="sm" onClick={() => setAdding(true)}>
        Add the {summaryFieldLabel(fieldName).toLowerCase()}
      </Button>
      {adding && (
        <EditSummaryDialog
          operationId={operationId}
          fieldName={fieldName}
          field={undefined}
          initial=""
          onClose={() => setAdding(false)}
          onStale={onStale}
        />
      )}
    </li>
  );
}

function Ready({ summary, onStale }: { onStale: () => void; summary: Summary }) {
  const hostNames = useHostNames();
  const me = useSession().data?.user.id;
  const missing = NARRATIVE_FIELDS.filter(
    (name) => !summary.fields.some((field) => field.field === name),
  );
  return (
    <Section
      title={TITLE}
      description="What this operation found and did. Domovoi fills some fields; the model drafted the rest until you confirm or edit it."
    >
      <Stack gap={3}>
        <ul className={styles.fields} aria-label="Summary fields">
          {summary.fields.map((field) => (
            <SummaryFieldRow
              key={field.field}
              operationId={summary.operation_id}
              field={field}
              hostNames={hostNames}
              me={me}
              onStale={onStale}
            />
          ))}
          {missing.map((name) => (
            <MissingField
              key={name}
              operationId={summary.operation_id}
              fieldName={name}
              onStale={onStale}
            />
          ))}
        </ul>
        <SummaryHistory summary={summary} hostNames={hostNames} me={me} />
      </Stack>
    </Section>
  );
}

/**
 * The resolution summary of a finished operation: each field with the source of its text,
 * Confirm and Edit for the narrative ones, and the edit history. Shows nothing while the
 * operation is open and a progress line while the summary is being written; the operation's
 * live updates refresh it.
 */
export function ResolutionSummary({ operationId }: { operationId: string }) {
  const query = useResolutionSummary(operationId);
  if (query.data?.state === "not_ready") {
    return null;
  }
  return (
    <Visible category="P0" id="experience.summary">
      <QueryBoundary
        query={query}
        context="load the resolution summary"
        loading={<Skeleton shape="rows" count={3} />}
      >
        {(summary) =>
          summary.state === "drafting" ? (
            <Section title={TITLE}>
              <Progress label="Domovoi is writing the summary…" />
            </Section>
          ) : (
            <Ready summary={summary} onStale={() => void query.refetch()} />
          )
        }
      </QueryBoundary>
    </Visible>
  );
}
