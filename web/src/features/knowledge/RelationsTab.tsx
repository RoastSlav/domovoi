import { useState } from "react";
import { useRelations } from "../../api/hooks/knowledge";
import { AllHostsNote } from "../../app/AllHostsNote";
import {
  Button,
  Checkbox,
  EmptyState,
  Field,
  QueryBoundary,
  Select,
  Skeleton,
  Visible,
} from "../../components";
import { RELATION_KINDS } from "../../design/knowledgeText";
import { RelationForm } from "./RelationForm";
import { RelationsTable } from "./RelationsTable";
import { useSubjectLabels } from "./useSubjectLabels";
import styles from "./knowledge.module.css";

function RelationList({ kind, retracted }: { kind: string; retracted: boolean }) {
  const names = useSubjectLabels();
  const relations = useRelations({
    kind: kind || undefined,
    include_retracted: retracted ? true : undefined,
  });
  return (
    <QueryBoundary
      query={relations}
      context="load relations"
      loading={<Skeleton shape="rows" count={4} />}
      isEmpty={(data) => data.pages.every((page) => page.items.length === 0)}
      empty={
        <EmptyState icon="info" title="No relations yet">
          <p>
            A relation links two things, such as a container and the host it runs on. Add the ones
            you know.
          </p>
        </EmptyState>
      }
    >
      {(data) => (
        <>
          <RelationsTable
            caption="Relations"
            names={names}
            relations={data.pages.flatMap((page) => page.items)}
          />
          {relations.hasNextPage && (
            <div className={styles.more}>
              <Button
                loading={relations.isFetchingNextPage}
                onClick={() => void relations.fetchNextPage()}
              >
                Load more
              </Button>
            </div>
          )}
        </>
      )}
    </QueryBoundary>
  );
}

/** Relations between subjects, for every host, with retracted ones shown only when asked for. */
export function RelationsTab() {
  const [kind, setKind] = useState("");
  const [retracted, setRetracted] = useState(false);
  const [adding, setAdding] = useState(false);
  return (
    <>
      <div className={styles.note}>
        <AllHostsNote>Relations and entities are shown for every host.</AllHostsNote>
      </div>
      <div className={styles.toolbar}>
        <Visible category="P1" id="knowledge.filters">
          <Field label="Kind">
            <Select value={kind} onChange={(event) => setKind(event.target.value)}>
              <option value="">Any kind</option>
              {Object.entries(RELATION_KINDS).map(([key, phrase]) => (
                <option key={key} value={key}>
                  {phrase}
                </option>
              ))}
            </Select>
          </Field>
          <Checkbox label="Show retracted" checked={retracted} onChange={setRetracted} />
        </Visible>
        <div className={styles.toolbarEnd}>
          <Visible category="P1" id="knowledge.add">
            <Button variant="primary" icon="plus" onClick={() => setAdding(true)}>
              Add a relation
            </Button>
          </Visible>
        </div>
      </div>
      <Visible category="P0" id="knowledge.relations">
        <RelationList kind={kind} retracted={retracted} />
      </Visible>
      {adding && <RelationForm onClose={() => setAdding(false)} />}
    </>
  );
}
