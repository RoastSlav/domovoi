import { useHosts } from "../../api/hooks/hosts";
import {
  Disclosure,
  Field,
  NumberInput,
  Section,
  Select,
  Stack,
  TextInput,
} from "../../components";
import { shadowStatusMeta } from "../../design/tone";
import { usePolicyChoices } from "./policyChoices";
import { ChoiceCards } from "./ChoiceCards";
import styles from "./simulator.module.css";

/** The filters and size of a simulation, as the form holds them before Run. */
export interface FilterValues {
  /** A host id; null follows the selected host. */
  host: string | null;
  /** Part of an action name; empty matches every action. */
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
/** Up to this many hosts are shown as choices; more go in a list. */
const HOST_CHOICES = 4;

/** How many filters differ from what the form starts with. */
export function countFilters(values: FilterValues): number {
  return [
    (values.host ?? "") !== "",
    values.contract.trim() !== "",
    values.status !== INITIAL_FILTERS.status,
    values.since !== "",
    values.until !== "",
    values.limit !== INITIAL_FILTERS.limit,
  ].filter(Boolean).length;
}

function HostField({ value, onChange }: { onChange: (host: string) => void; value: string }) {
  const hosts = useHosts().data ?? [];
  if (hosts.length > 0 && hosts.length <= HOST_CHOICES) {
    return (
      <ChoiceCards
        legend="Host"
        value={value}
        onChange={onChange}
        choices={[
          { value: "", label: "All hosts" },
          ...hosts.map((h) => ({ value: h.id, label: h.name })),
        ]}
      />
    );
  }
  return (
    <Field label="Host">
      <Select value={value} onChange={(e) => onChange(e.target.value)}>
        <option value="">All hosts</option>
        {hosts.map((h) => (
          <option key={h.id} value={h.id}>
            {h.name}
          </option>
        ))}
      </Select>
    </Field>
  );
}

function ActionField({
  hostId,
  value,
  onChange,
}: {
  hostId: string;
  onChange: (contract: string) => void;
  value: string;
}) {
  const choices = usePolicyChoices(hostId);
  const listed = value === "" || choices.actions.some((a) => a.contract === value);
  const unreadable = choices.failed || (!choices.loading && choices.actions.length === 0);
  return (
    <Stack gap={2}>
      <Field label="Action" hint="Pick from what the host offers.">
        <Select value={value} onChange={(e) => onChange(e.target.value)}>
          <option value="">Any action</option>
          {!listed && <option value={value}>{value}</option>}
          {choices.actions.map((a) => (
            <option key={a.contract} value={a.contract}>
              {`${a.title} (${a.contract})`}
            </option>
          ))}
        </Select>
      </Field>
      <Disclosure label="Type a name that is not listed" reveal={unreadable || !listed}>
        <Field label="Action name contains (optional)">
          <TextInput mono value={value} onChange={(e) => onChange(e.target.value)} />
        </Field>
      </Disclosure>
    </Stack>
  );
}

/**
 * The optional filters of a simulation, in a section of their own under the required choices.
 * Every control says it is optional; the section says how many differ from the starting ones.
 * @param values The filters held by the page.
 * @param onChange Receives the part that changed.
 */
export function SimulatorFilters({
  values,
  onChange,
}: {
  onChange: (patch: Partial<FilterValues>) => void;
  values: FilterValues;
}) {
  const count = countFilters(values);
  const hosts = useHosts().data ?? [];
  const host = hosts.find((h) => h.id === (values.host ?? ""))?.name ?? "All hosts";
  const summary = [
    host,
    shadowStatusMeta(values.status).label,
    values.contract.trim() !== "" ? values.contract.trim() : null,
    values.since !== "" || values.until !== ""
      ? `${values.since || "any start"} to ${values.until || "now"}`
      : null,
    `up to ${values.limit} results`,
  ]
    .filter(Boolean)
    .join(" · ");
  return (
    <Section title="Which decisions" description="Optional. Change it to narrow the run.">
      <Disclosure label={summary} count={count > 0 ? count : undefined} reveal={false}>
        <div className={`${styles.group} ${styles.optional}`}>
          <div className={styles.wide}>
            <HostField value={values.host ?? ""} onChange={(host) => onChange({ host })} />
          </div>
          <div className={styles.wide}>
            <ActionField
              hostId={values.host ?? ""}
              value={values.contract}
              onChange={(contract) => onChange({ contract })}
            />
          </div>
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
        </div>
      </Disclosure>
    </Section>
  );
}
