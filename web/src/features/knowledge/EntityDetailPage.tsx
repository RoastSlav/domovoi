import { useCallback, useState } from "react";
import { Link, useNavigate, useParams, useSearchParams } from "react-router";
import { useEntities, useFacts, useRelations } from "../../api/hooks/knowledge";
import type { KnowledgeEntity } from "../../api/types";
import { usePageName } from "../../app/PageTitle";
import {
  Button,
  Cluster,
  EmptyState,
  FilterChips,
  PageHeader,
  QueryBoundary,
  Section,
  Skeleton,
  Stack,
  Tag,
  Visible,
  VisibilityPage,
} from "../../components";
import { AssertFactDialog } from "./AssertFactDialog";
import { DeleteEntityDialog } from "./DeleteEntityDialog";
import { EntityForm } from "./EntityForm";
import {
  readFactView,
  STATUS_CHIPS,
  statusParam,
  writeFactView,
  type FactView,
} from "./factFilters";
import { FactsTable } from "./FactsTable";
import { RelationsTable } from "./RelationsTable";
import { useSubjectLabels } from "./useSubjectLabels";

function EntityFacts({ entity }: { entity: KnowledgeEntity }) {
  const names = useSubjectLabels();
  const [params, setParams] = useSearchParams();
  const [adding, setAdding] = useState(false);
  const view = readFactView(params);
  const change = useCallback(
    (next: Partial<FactView>) =>
      setParams((previous) => writeFactView(previous, next), { replace: true }),
    [setParams],
  );
  const facts = useFacts({ subject: `entity:${entity.id}`, status: statusParam(view.status) });
  return (
    <Visible category="P1" id="entity.facts">
      <Section
        title="Facts about it"
        actions={
          <Visible category="P1" id="knowledge.add">
            <Button icon="plus" onClick={() => setAdding(true)}>
              Add a fact
            </Button>
          </Visible>
        }
      >
        <Stack gap={3}>
          <FilterChips
            label="Status"
            options={STATUS_CHIPS}
            value={view.status}
            onChange={(status) => change({ status })}
          />
          <Visible category="P0" id="knowledge.facts">
            <QueryBoundary
              query={facts}
              context="load facts"
              loading={<Skeleton shape="rows" count={3} />}
              isEmpty={(data) => data.pages.every((page) => page.items.length === 0)}
              empty={<EmptyState icon="info" title="No facts match" />}
            >
              {(data) => (
                <>
                  <FactsTable
                    caption={`Facts about ${entity.name}`}
                    facts={data.pages.flatMap((page) => page.items)}
                    names={names}
                  />
                  {facts.hasNextPage && (
                    <Button
                      loading={facts.isFetchingNextPage}
                      onClick={() => void facts.fetchNextPage()}
                    >
                      Load more
                    </Button>
                  )}
                </>
              )}
            </QueryBoundary>
          </Visible>
        </Stack>
      </Section>
      {adding && (
        <AssertFactDialog
          onClose={() => setAdding(false)}
          preset={{ subject: `entity:${entity.id}` }}
        />
      )}
    </Visible>
  );
}

function EntityRelations({ entity }: { entity: KnowledgeEntity }) {
  const names = useSubjectLabels();
  const relations = useRelations({ subject: `entity:${entity.id}` });
  return (
    <Visible category="P1" id="entity.relations">
      <Section title="Relations">
        <Visible category="P0" id="knowledge.relations">
          <QueryBoundary
            query={relations}
            context="load relations"
            loading={<Skeleton shape="rows" count={2} />}
            isEmpty={(data) => data.pages.every((page) => page.items.length === 0)}
            empty={<EmptyState icon="info" title="No relations yet" />}
          >
            {(data) => (
              <>
                <RelationsTable
                  caption={`Relations of ${entity.name}`}
                  names={names}
                  relations={data.pages.flatMap((page) => page.items)}
                />
                {relations.hasNextPage && (
                  <Button
                    loading={relations.isFetchingNextPage}
                    onClick={() => void relations.fetchNextPage()}
                  >
                    Load more
                  </Button>
                )}
              </>
            )}
          </QueryBoundary>
        </Visible>
      </Section>
    </Visible>
  );
}

function Body({ entity }: { entity: KnowledgeEntity }) {
  const navigate = useNavigate();
  const [renaming, setRenaming] = useState(false);
  const [deleting, setDeleting] = useState(false);
  return (
    <>
      <PageHeader
        title={entity.name}
        docs="concepts/knowledge.html"
        topic="knowledge"
        meta={<Tag>{entity.kind}</Tag>}
        actions={
          <Visible category="P0" id="entity.actions">
            <Cluster gap={2}>
              <Button onClick={() => setRenaming(true)}>Rename</Button>
              <Button onClick={() => setDeleting(true)}>Delete</Button>
            </Cluster>
          </Visible>
        }
      />
      <Stack gap={6}>
        <EntityFacts entity={entity} />
        <EntityRelations entity={entity} />
      </Stack>
      {renaming && <EntityForm entity={entity} onClose={() => setRenaming(false)} />}
      {deleting && (
        <DeleteEntityDialog
          entity={entity}
          onClose={() => setDeleting(false)}
          onDeleted={() => void navigate("/knowledge?tab=entities")}
        />
      )}
    </>
  );
}

/** One entity the owner named: its facts and relations, with rename and delete. */
export function EntityDetailPage() {
  const { id = "" } = useParams();
  const entities = useEntities();
  const entity = entities.data?.find((candidate) => candidate.id === id);
  usePageName(entity?.name);
  return (
    <VisibilityPage page="knowledge-detail">
      <Visible category="P0" id="entity.state">
        {entities.data !== undefined && entity === undefined ? (
          <>
            <PageHeader title="Entity" />
            <EmptyState
              icon="info"
              title="No such entity"
              action={<Link to="/knowledge?tab=entities">Back to entities</Link>}
            >
              This entity does not exist, or it was deleted.
            </EmptyState>
          </>
        ) : (
          <QueryBoundary
            query={{ ...entities, data: entity }}
            context="load the entity"
            loading={
              <>
                <PageHeader title="Entity" />
                <Skeleton shape="rows" count={3} />
              </>
            }
          >
            {(found) => <Body entity={found} />}
          </QueryBoundary>
        )}
      </Visible>
    </VisibilityPage>
  );
}
