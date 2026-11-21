import { useMemo, useState } from "react";
import { useEntities } from "../../api/hooks/knowledge";
import type { KnowledgeEntity } from "../../api/types";
import { AllHostsNote } from "../../app/AllHostsNote";
import {
  Button,
  DataTable,
  EmptyState,
  QueryBoundary,
  RelativeTime,
  Skeleton,
  Tag,
  Visible,
  type Column,
} from "../../components";
import { DeleteEntityDialog } from "./DeleteEntityDialog";
import { EntityForm } from "./EntityForm";
import styles from "./knowledge.module.css";

/** The entities the owner named, with rename and delete; each name opens the entity's page. */
export function EntitiesTab() {
  const entities = useEntities();
  const [adding, setAdding] = useState(false);
  const [renaming, setRenaming] = useState<KnowledgeEntity | undefined>(undefined);
  const [deleting, setDeleting] = useState<KnowledgeEntity | undefined>(undefined);
  const columns = useMemo<Column<KnowledgeEntity>[]>(
    () => [
      { key: "name", label: "Name", cell: (entity) => entity.name },
      { key: "kind", label: "Kind", cell: (entity) => <Tag>{entity.kind}</Tag> },
      {
        key: "created",
        label: "Created",
        cell: (entity) => <RelativeTime at={entity.created_at} />,
      },
      {
        key: "actions",
        label: "Actions",
        hideLabel: true,
        cell: (entity) => (
          <span className={styles.actions}>
            <Button size="sm" onClick={() => setRenaming(entity)}>
              Rename
            </Button>
            <Button size="sm" onClick={() => setDeleting(entity)}>
              Delete
            </Button>
          </span>
        ),
      },
    ],
    [],
  );
  return (
    <>
      <div className={styles.note}>
        <AllHostsNote>Relations and entities are shown for every host.</AllHostsNote>
      </div>
      <div className={styles.toolbar}>
        <div className={styles.toolbarEnd}>
          <Visible category="P1" id="knowledge.add">
            <Button variant="primary" icon="plus" onClick={() => setAdding(true)}>
              Add an entity
            </Button>
          </Visible>
        </div>
      </div>
      <Visible category="P0" id="knowledge.entities">
        <QueryBoundary
          query={entities}
          context="load entities"
          loading={<Skeleton shape="rows" count={3} />}
          isEmpty={(data) => data.length === 0}
          empty={
            <EmptyState icon="info" title="No entities yet">
              <p>
                An entity is a thing you name yourself, such as a VPN or a storage pool, so facts
                can be about it.
              </p>
            </EmptyState>
          }
        >
          {(data) => (
            <DataTable
              caption="Entities"
              columns={columns}
              rows={data}
              rowKey={(entity) => entity.id}
              rowHref={(entity) => `/knowledge/entities/${encodeURIComponent(entity.id)}`}
            />
          )}
        </QueryBoundary>
      </Visible>
      {adding && <EntityForm onClose={() => setAdding(false)} />}
      {renaming && <EntityForm entity={renaming} onClose={() => setRenaming(undefined)} />}
      {deleting && <DeleteEntityDialog entity={deleting} onClose={() => setDeleting(undefined)} />}
    </>
  );
}
