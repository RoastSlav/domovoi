import type { ReactNode } from "react";
import type { PolicyOverview, PolicyVersionSummary } from "../../api/types";
import {
  Button,
  Field,
  InlineMessage,
  SegmentedControl,
  Section,
  Select,
  Stack,
  Visible,
} from "../../components";
import type { DescribedError } from "../../design/messages";
import { SimulatorChoices, type SimulatorChoice } from "./SimulatorChoices";
import { SimulatorFilters, type FilterValues } from "./SimulatorFilters";
import { SIMULATION_CURRENT, SIMULATION_RECORDED } from "./policyText";
import styles from "./simulator.module.css";
import type { SimulatorRules, SimulatorState } from "./simulatorState";

export { INITIAL_FILTERS, type FilterValues } from "./SimulatorFilters";

const rulesValue = (r: SimulatorRules | null): string =>
  r === null ? "" : r.source === "draft" ? "draft" : `version-${r.number}`;

function RulesField({
  value,
  draft,
  versions,
  more,
  active,
  onChange,
}: {
  active: number | undefined;
  draft: PolicyOverview["draft"];
  more: boolean;
  onChange: (rules: SimulatorRules) => void;
  value: SimulatorRules | null;
  versions: readonly PolicyVersionSummary[];
}) {
  const current = value?.source === "version" ? value.number : undefined;
  const older = versions.map((v) => v.number).filter((n) => n !== active);
  if (current !== undefined && current !== active && !older.includes(current)) older.push(current);
  const change = (text: string) =>
    onChange(
      text === "draft"
        ? { source: "draft" }
        : { source: "version", number: Number(text.replace("version-", "")) },
    );
  const choices: SimulatorChoice[] = [
    {
      value: "draft",
      label: draft ? `Draft, revision ${draft.revision}` : "Draft (none to simulate)",
      disabled: !draft,
      ...(draft ? {} : { hint: "There is no draft yet." }),
    },
  ];
  if (active !== undefined)
    choices.push({ value: `version-${active}`, label: `Active version ${active}` });
  const chosen = value === null ? "" : rulesValue(value);
  return (
    <Stack gap={3}>
      <SimulatorChoices
        legend="Test these rules"
        choices={choices}
        value={chosen}
        onChange={change}
        hint={more ? "Older versions are not listed." : "The rules to run the decisions through."}
      />
      {older.length > 0 && (
        <Field label="Older version (optional)">
          <Select
            value={current !== undefined && older.includes(current) ? `version-${current}` : ""}
            onChange={(e) => e.target.value !== "" && change(e.target.value)}
          >
            <option value="">Choose an older version</option>
            {older.map((n) => (
              <option key={n} value={`version-${n}`}>{`Version ${n}`}</option>
            ))}
          </Select>
        </Field>
      )}
    </Stack>
  );
}

/**
 * The simulator's settings: which rules, what to compare them with, which inputs, and, behind
 * a toggle, which decisions to take. Nothing is sent until Run simulation.
 * @param state Rules, selection and inputs as the address holds them.
 * @param onState Changes the address.
 * @param values The filters, held by the page.
 * @param onValues Changes the filters.
 * @param onRun Starts the simulation.
 */
export function SimulatorForm({
  state,
  rules,
  overview,
  versions,
  moreVersions,
  values,
  onState,
  onValues,
  onRun,
  pending,
  error,
  step,
}: {
  error: DescribedError | undefined;
  moreVersions: boolean;
  onRun: () => void;
  onState: (patch: Partial<SimulatorState>) => void;
  onValues: (patch: Partial<FilterValues>) => void;
  overview: PolicyOverview;
  pending: boolean;
  rules: SimulatorRules | null;
  state: SimulatorState;
  /** Names the one step being simulated, with a way to drop it. */
  step?: ReactNode;
  values: FilterValues;
  versions: readonly PolicyVersionSummary[];
}) {
  return (
    <Visible category="P1" id="simulator.form">
      <form
        className={styles.form}
        onSubmit={(event) => {
          event.preventDefault();
          if (rules !== null) onRun();
        }}
        noValidate
      >
        <Stack gap={4}>
          {step}
          <Section title="What to test" description="All three are needed to run.">
            <div className={styles.group}>
              <RulesField
                value={rules}
                draft={overview.draft}
                versions={versions}
                more={moreVersions}
                active={overview.active?.number}
                onChange={(next) => onState({ rules: next })}
              />
              <Field label="Against" hint="Which recorded decisions to compare with.">
                <SegmentedControl
                  label="Against"
                  value={state.selection}
                  onChange={(selection) => onState({ selection })}
                  options={[
                    { value: "enforced", label: "Active policy" },
                    { value: "shadow", label: "Shadow rules" },
                    { value: "both", label: "Both" },
                  ]}
                />
              </Field>
              <Field
                label="Using"
                hint={state.inputs === "current" ? SIMULATION_CURRENT : SIMULATION_RECORDED}
              >
                <SegmentedControl
                  label="Using"
                  value={state.inputs}
                  onChange={(inputs) => onState({ inputs })}
                  options={[
                    { value: "recorded", label: "Recorded" },
                    { value: "current", label: "Current" },
                  ]}
                />
              </Field>
            </div>
          </Section>
          <SimulatorFilters values={values} onChange={onValues} />
          {error && (
            <InlineMessage tone="danger" title={error.title}>
              {error.detail}
            </InlineMessage>
          )}
          <div className={styles.run}>
            <Button variant="primary" type="submit" loading={pending}>
              Run simulation
            </Button>
          </div>
        </Stack>
      </form>
    </Visible>
  );
}
