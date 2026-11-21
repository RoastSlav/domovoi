import { useState } from "react";
import { useHosts } from "../../api/hooks/hosts";
import { useEntities } from "../../api/hooks/knowledge";
import { Field, Select, TextInput } from "../../components";
import type { SubjectKind } from "../../design/knowledgeText";
import { formatSubject, parseSubject, type ParsedSubject } from "./subjects";
import styles from "./knowledge.module.css";

const KIND_LABELS: Record<SubjectKind, string> = {
  host: "A host",
  container: "A container",
  unit: "A service",
  entity: "Something you named",
};

/** Whether `subject` has everything its kind needs to be sent. */
function complete(subject: ParsedSubject): boolean {
  if (subject.kind === "entity") {
    return Boolean(subject.id);
  }
  if (subject.kind === "host") {
    return Boolean(subject.host);
  }
  return Boolean(subject.host) && Boolean(subject.name?.trim());
}

/**
 * Picks what a fact or relation is about: its kind, then a host, a host and a name, or an entity.
 * Reports the reference (`host:<id>`) once it is complete and an empty string until then.
 * `kinds` limits what may be chosen; a field that never changes kind can pass one.
 */
export function SubjectField({
  label,
  kinds,
  value,
  onChange,
}: {
  kinds: readonly SubjectKind[];
  label: string;
  onChange: (reference: string) => void;
  value: string;
}) {
  const hosts = useHosts(false);
  const entities = useEntities();
  const [state, setState] = useState<ParsedSubject>(
    () => parseSubject(value) ?? { kind: kinds[0] ?? "host" },
  );
  const kind = kinds.includes(state.kind) ? state.kind : (kinds[0] ?? "host");
  const update = (change: Partial<ParsedSubject>) => {
    const next = { ...state, kind, ...change };
    setState(next);
    onChange(complete(next) ? formatSubject(next) : "");
  };
  const hostSelect = (
    <Field label="Host">
      <Select value={state.host ?? ""} onChange={(event) => update({ host: event.target.value })}>
        <option value="">Choose a host</option>
        {(hosts.data ?? []).map((host) => (
          <option key={host.id} value={host.id}>
            {host.name}
          </option>
        ))}
      </Select>
    </Field>
  );
  return (
    <div role="group" aria-label={label} className={styles.pair}>
      {kinds.length > 1 && (
        <Field label={`${label} is`}>
          <Select
            value={kind}
            onChange={(event) => {
              const next: ParsedSubject = { kind: event.target.value as SubjectKind };
              setState(next);
              onChange("");
            }}
          >
            {kinds.map((option) => (
              <option key={option} value={option}>
                {KIND_LABELS[option]}
              </option>
            ))}
          </Select>
        </Field>
      )}
      {kind !== "entity" && hostSelect}
      {(kind === "container" || kind === "unit") && (
        <Field label={kind === "container" ? "Container name" : "Service name"}>
          <TextInput
            value={state.name ?? ""}
            autoComplete="off"
            onChange={(event) => update({ name: event.target.value })}
          />
        </Field>
      )}
      {kind === "entity" && (
        <Field label="Entity">
          <Select value={state.id ?? ""} onChange={(event) => update({ id: event.target.value })}>
            <option value="">Choose an entity</option>
            {(entities.data ?? []).map((entity) => (
              <option key={entity.id} value={entity.id}>
                {entity.name}
              </option>
            ))}
          </Select>
        </Field>
      )}
    </div>
  );
}
