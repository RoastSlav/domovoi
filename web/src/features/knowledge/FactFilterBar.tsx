import { useEffect, useRef, useState } from "react";
import {
  Button,
  Field,
  FilterChips,
  Select,
  TextInput,
  Visible,
  useDebounced,
} from "../../components";
import { PREDICATES, SOURCES } from "../../design/knowledgeText";
import { STATUS_CHIPS, type FactView } from "./factFilters";
import styles from "./knowledge.module.css";

/** How long typing must pause before a search runs. */
export const FACT_SEARCH_DEBOUNCE_MS = 300;

/**
 * The status chips, the source and kind selects, the search box and the Add button. The state
 * lives in the address: `view` is read from it and `onChange` writes the change back.
 */
export function FactFilterBar({
  view,
  onChange,
  onAdd,
}: {
  onAdd: () => void;
  onChange: (change: Partial<FactView>) => void;
  view: FactView;
}) {
  const [text, setText] = useState(view.q);
  // The search text the address holds or was last sent to it: it tells our own write from
  // someone else's change (Clear filters), so typing is never overwritten.
  const sent = useRef(view.q);
  const settled = useDebounced(text.trim(), FACT_SEARCH_DEBOUNCE_MS);
  const wanted = text.trim() === "" ? "" : settled;
  useEffect(() => {
    if (view.q !== sent.current) {
      sent.current = view.q;
      setText(view.q);
    }
  }, [view.q]);
  useEffect(() => {
    if (wanted !== sent.current) {
      sent.current = wanted;
      onChange({ q: wanted });
    }
  }, [wanted, onChange]);

  return (
    <div className={styles.toolbar} role="search">
      <Visible category="P1" id="knowledge.filters">
        <FilterChips
          label="Status"
          options={STATUS_CHIPS}
          value={view.status}
          onChange={(status) => onChange({ status })}
        />
        <Field label="Source">
          <Select
            value={view.source}
            onChange={(event) => onChange({ source: event.target.value })}
          >
            <option value="">Any source</option>
            {Object.entries(SOURCES).map(([key, source]) => (
              <option key={key} value={key}>
                {source.label}
              </option>
            ))}
          </Select>
        </Field>
        <Field label="Kind">
          <Select
            value={view.predicate}
            onChange={(event) => onChange({ predicate: event.target.value })}
          >
            <option value="">Any kind</option>
            {Object.entries(PREDICATES).map(([key, predicate]) => (
              <option key={key} value={key}>
                {predicate.label}
              </option>
            ))}
          </Select>
        </Field>
        <div className={styles.search}>
          <Field label="Search">
            <TextInput
              type="search"
              value={text}
              onChange={(event) => setText(event.target.value)}
              placeholder="Search facts"
            />
          </Field>
        </div>
      </Visible>
      <div className={styles.toolbarEnd}>
        <Visible category="P1" id="knowledge.add">
          <Button variant="primary" icon="plus" onClick={onAdd}>
            Add a fact
          </Button>
        </Visible>
      </div>
    </div>
  );
}
