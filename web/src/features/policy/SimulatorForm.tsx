import type { ReactNode } from "react";
import { useHosts } from "../../api/hooks/hosts";
import type { PolicyOverview, PolicyVersionSummary } from "../../api/types";
import {
  Button,
  Field,
  Grid,
  InlineMessage,
  NumberInput,
  SegmentedControl,
  Select,
  Stack,
  TextInput,
  Visible,
} from "../../components";
import type { DescribedError } from "../../design/messages";
import { shadowStatusMeta } from "../../design/tone";
import { SIMULATION_CURRENT, SIMULATION_RECORDED } from "./policyText";
import type { SimulatorRules, SimulatorState } from "./simulatorState";

/** The filters and size of a simulation, as the form holds them before Run. */
export interface FilterValues {
  /** A host id; null follows the selected host. */
  host: string | null;
  contract: string;
  status: string;
  /** A date as `YYYY-MM-DD`, or empty. */
  since: string;
  until: string;
  limit: string;
}

/** What the form starts with: recorded decisions, up to 200, any host the selector names. */
export const INITIAL_FILTERS: FilterValues = {
  host: null,
  contract: "",
  status: "evaluated",
  since: "",
  until: "",
  limit: "200",
};

const STATUSES = ["evaluated", "unevaluable", "skipped"];

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
  const numbers = versions.map((v) => v.number);
  if (current !== undefined && !numbers.includes(current)) numbers.push(current);
  const change = (text: string) =>
    onChange(
      text === "draft"
        ? { source: "draft" }
        : { source: "version", number: Number(text.replace("version-", "")) },
    );
  return (
    <Field
      label="Rules"
      hint={more ? "Older versions are not listed." : "The rules to run the decisions through."}
    >
      <Select value={rulesValue(value)} onChange={(e) => change(e.target.value)}>
        {value === null && <option value="">Choose rules</option>}
        <option value="draft" disabled={!draft}>
          {draft ? `Draft, revision ${draft.revision}` : "Draft (none to simulate)"}
        </option>
        {numbers.map((n) => (
          <option key={n} value={`version-${n}`}>
            {`Version ${n}${n === active ? " (active)" : ""}`}
          </option>
        ))}
      </Select>
    </Field>
  );
}

function Filters({
  values,
  onChange,
}: {
  onChange: (patch: Partial<FilterValues>) => void;
  values: FilterValues;
}) {
  const hosts = useHosts().data ?? [];
  return (
    <Grid min="xs" gap={4}>
      <Field label="Host">
        <Select value={values.host ?? ""} onChange={(e) => onChange({ host: e.target.value })}>
          <option value="">All hosts</option>
          {hosts.map((h) => (
            <option key={h.id} value={h.id}>
              {h.name}
            </option>
          ))}
        </Select>
      </Field>
      <Field label="Capability contains">
        <TextInput
          mono
          value={values.contract}
          onChange={(e) => onChange({ contract: e.target.value })}
        />
      </Field>
      <Field label="Status">
        <Select value={values.status} onChange={(e) => onChange({ status: e.target.value })}>
          <option value="">Any status</option>
          {STATUSES.map((s) => (
            <option key={s} value={s}>
              {shadowStatusMeta(s).label}
            </option>
          ))}
        </Select>
      </Field>
      <Field label="From (UTC)">
        <TextInput
          type="date"
          value={values.since}
          onChange={(e) => onChange({ since: e.target.value })}
        />
      </Field>
      <Field label="To (UTC)">
        <TextInput
          type="date"
          value={values.until}
          onChange={(e) => onChange({ until: e.target.value })}
        />
      </Field>
      <Field label="Most results" hint="Between 1 and 500.">
        <NumberInput
          min={1}
          max={500}
          value={values.limit}
          onChange={(e) => onChange({ limit: e.target.value })}
        />
      </Field>
    </Grid>
  );
}

/**
 * The simulator's settings: which rules, which side of the recorded decisions, which inputs,
 * and which decisions to take. Nothing is sent until Run simulation.
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
        onSubmit={(event) => {
          event.preventDefault();
          if (rules !== null) onRun();
        }}
        noValidate
      >
        <Stack gap={4}>
          {step}
          <Grid min="sm" gap={4}>
            <RulesField
              value={rules}
              draft={overview.draft}
              versions={versions}
              more={moreVersions}
              active={overview.active?.number}
              onChange={(next) => onState({ rules: next })}
            />
            <Field label="Which rules">
              <SegmentedControl
                label="Which rules"
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
              label="Inputs"
              hint={state.inputs === "current" ? SIMULATION_CURRENT : SIMULATION_RECORDED}
            >
              <SegmentedControl
                label="Inputs"
                value={state.inputs}
                onChange={(inputs) => onState({ inputs })}
                options={[
                  { value: "recorded", label: "Recorded" },
                  { value: "current", label: "Current" },
                ]}
              />
            </Field>
          </Grid>
          <Filters values={values} onChange={onValues} />
          {error && (
            <InlineMessage tone="danger" title={error.title}>
              {error.detail}
            </InlineMessage>
          )}
          <div>
            <Button variant="primary" type="submit" loading={pending}>
              Run simulation
            </Button>
          </div>
        </Stack>
      </form>
    </Visible>
  );
}
