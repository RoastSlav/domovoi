import { Field, SegmentedControl, TextInput, Visible } from "../../components";
import type { ExperienceScope } from "./experienceParams";
import styles from "./experience.module.css";

const SCOPES = [
  { value: "host", label: "Selected host" },
  { value: "global", label: "All hosts" },
] as const;

/** The scope control and the search box of the experience page. */
export function ExperienceFilters({
  scope,
  text,
  onScope,
  onText,
}: {
  onScope: (scope: ExperienceScope) => void;
  onText: (text: string) => void;
  scope: ExperienceScope;
  text: string;
}) {
  return (
    <Visible category="P1" id="experience.filters">
      <div className={styles.filters}>
        <SegmentedControl label="Search scope" options={SCOPES} value={scope} onChange={onScope} />
        <Field label="Search">
          <TextInput
            type="search"
            value={text}
            placeholder="Words from the goal, problem, cause or fix"
            onChange={(event) => onText(event.target.value)}
          />
        </Field>
      </div>
    </Visible>
  );
}
