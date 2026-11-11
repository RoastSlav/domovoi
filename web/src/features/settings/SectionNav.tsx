import { Link } from "react-router";
import styles from "./layout.module.css";
import type { SectionSpec } from "./sections";

/** What the list shows beside a section: its unsaved changes and its problems. */
export interface SectionState {
  unsaved: number;
  problems: boolean;
}

function Dots({ state }: { state: SectionState | undefined }) {
  if (!state || (state.unsaved === 0 && !state.problems)) {
    return null;
  }
  return (
    <span className={styles.dots}>
      {state.unsaved > 0 && (
        <>
          <span className={styles.dot} data-tone="warning" aria-hidden="true" />
          <span className="visually-hidden">unsaved changes</span>
        </>
      )}
      {state.problems && (
        <>
          <span className={styles.dot} data-tone="danger" aria-hidden="true" />
          <span className="visually-hidden">has problems</span>
        </>
      )}
    </span>
  );
}

/**
 * The list of sections. A warning dot marks a section with unsaved changes and a danger dot
 * one with problems; each dot has words for assistive technology, so colour is never alone.
 */
export function SectionNav({
  sections,
  selected,
  states,
}: {
  sections: readonly SectionSpec[];
  selected: string | undefined;
  states: ReadonlyMap<string, SectionState>;
}) {
  return (
    <nav aria-label="Settings sections">
      <ul className={styles.nav}>
        {sections.map((section) => (
          <li key={section.id}>
            <Link
              to={`/settings/${section.id}`}
              className={styles.link}
              aria-current={section.id === selected ? "page" : undefined}
            >
              {section.title}
              <Dots state={states.get(section.id)} />
            </Link>
          </li>
        ))}
      </ul>
    </nav>
  );
}
