import { Tag } from "../../../components";
import { ResultGroup } from "../ResultGroup";
import styles from "../search.module.css";
import { SEARCH_GROUPS, type SearchGroups } from "../useSearchGroups";

const VALUE_LIMIT = 120;

/** A fact's value as short text: a string as it is, anything else as compact JSON. */
export function factValue(value: unknown): string {
  const text = typeof value === "string" ? value : (JSON.stringify(value) ?? "");
  return text.length > VALUE_LIMIT ? `${text.slice(0, VALUE_LIMIT)}…` : text;
}

/** Facts the hub knows that match the text. They are shown, not linked: facts have no page here. */
export function FactsGroup({ state }: { state: SearchGroups["facts"] }) {
  return (
    <ResultGroup
      group={SEARCH_GROUPS[4]}
      state={state}
      rows={state.items.map((fact) => ({
        key: fact.id,
        node: (
          <>
            <span className={styles.line}>
              <strong>{fact.subject}</strong>
              <span>{fact.predicate}</span>
              <span>{factValue(fact.value)}</span>
              <Tag>{fact.source}</Tag>
            </span>
            {fact.status === "stale" && <span className={styles.muted}>May be out of date</span>}
          </>
        ),
      }))}
    />
  );
}
