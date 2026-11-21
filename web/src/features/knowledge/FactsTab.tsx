import { useCallback, useState } from "react";
import { useSearchParams } from "react-router";
import { useFactSearch, useFacts, type FactFilters } from "../../api/hooks/knowledge";
import { useSelectedHost } from "../../app/HostContext";
import {
  Button,
  EmptyState,
  InlineMessage,
  LinkButton,
  QueryBoundary,
  Skeleton,
  Text,
  Visible,
} from "../../components";
import { AssertFactDialog } from "./AssertFactDialog";
import { FactFilterBar } from "./FactFilterBar";
import { readFactView, statusParam, writeFactView, type FactView } from "./factFilters";
import { FactsTable } from "./FactsTable";
import { useSubjectLabels } from "./useSubjectLabels";
import styles from "./knowledge.module.css";

/** How many matches a search returns at most. */
const SEARCH_LIMIT = 50;

function filtersOf(view: FactView, host: string | undefined): FactFilters {
  return {
    host,
    predicate: view.predicate || undefined,
    source: view.source || undefined,
    status: statusParam(view.status),
  };
}

function Empty({ view, onClear }: { onClear: () => void; view: FactView }) {
  if (view.status === "retracted" && view.q === "" && !view.source && !view.predicate) {
    return <EmptyState icon="info" title="No retracted facts" />;
  }
  const filtered = view.status !== "live" || view.source || view.predicate || view.q;
  if (!filtered) {
    return (
      <EmptyState icon="info" title="No facts yet">
        <p>
          Domovoi records what it learns here, and you can add what you know. Facts it is not sure
          of wait for your word.
        </p>
      </EmptyState>
    );
  }
  return (
    <EmptyState
      icon="search"
      title="No facts match"
      action={<Button onClick={onClear}>Clear filters</Button>}
    >
      <p>Try a different status, source or search.</p>
    </EmptyState>
  );
}

function FactList({
  view,
  host,
  onClear,
}: {
  host: string | undefined;
  onClear: () => void;
  view: FactView;
}) {
  const names = useSubjectLabels();
  const searching = view.q.trim() !== "";
  const filters = filtersOf(view, host);
  const list = useFacts(filters, !searching);
  const search = useFactSearch({ ...filters, q: view.q }, searching);
  if (searching) {
    return (
      <QueryBoundary
        query={search}
        context="load facts"
        loading={<Skeleton shape="rows" count={5} />}
        isEmpty={(data) => data.items.length === 0}
        empty={<Empty view={view} onClear={onClear} />}
      >
        {(data) => (
          <>
            <FactsTable caption="Facts" facts={data.items} names={names} />
            {data.items.length >= SEARCH_LIMIT && (
              <Text muted block>
                Showing the best 50 matches. Narrow the search to see others.
              </Text>
            )}
          </>
        )}
      </QueryBoundary>
    );
  }
  return (
    <QueryBoundary
      query={list}
      context="load facts"
      loading={<Skeleton shape="rows" count={5} />}
      isEmpty={(data) => data.pages.every((page) => page.items.length === 0)}
      empty={<Empty view={view} onClear={onClear} />}
    >
      {(data) => (
        <>
          <FactsTable
            caption="Facts"
            facts={data.pages.flatMap((page) => page.items)}
            names={names}
          />
          {list.hasNextPage && (
            <div className={styles.more}>
              <Button loading={list.isFetchingNextPage} onClick={() => void list.fetchNextPage()}>
                Load more
              </Button>
            </div>
          )}
        </>
      )}
    </QueryBoundary>
  );
}

/** The facts list: filters, search, table and the note about proposals that wait. */
export function FactsTab({ waiting }: { waiting: number }) {
  const [params, setParams] = useSearchParams();
  const [host] = useSelectedHost();
  const [adding, setAdding] = useState(false);
  const view = readFactView(params);
  const change = useCallback(
    (next: Partial<FactView>) =>
      setParams((previous) => writeFactView(previous, next), { replace: true }),
    [setParams],
  );
  const proposals = new URLSearchParams(params);
  proposals.set("tab", "proposals");
  return (
    <>
      {waiting > 0 && (
        <Visible category="P0" id="knowledge.waiting-note">
          <div className={styles.note}>
            <InlineMessage
              tone="pending"
              action={<LinkButton to={`?${proposals.toString()}`}>Review proposals</LinkButton>}
            >
              {waiting === 1
                ? "1 fact inferred by Domovoi waits for your word."
                : `${waiting} facts inferred by Domovoi wait for your word.`}
            </InlineMessage>
          </div>
        </Visible>
      )}
      <FactFilterBar view={view} onChange={change} onAdd={() => setAdding(true)} />
      <Visible category="P0" id="knowledge.facts">
        <FactList
          view={view}
          host={host}
          onClear={() => change({ status: "live", source: "", predicate: "", q: "" })}
        />
      </Visible>
      {adding && <AssertFactDialog onClose={() => setAdding(false)} />}
    </>
  );
}
