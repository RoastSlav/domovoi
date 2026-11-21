import { Field, SegmentedControl, Text, TextInput, Textarea } from "../../components";
import { predicateInfo } from "../../design/knowledgeText";
import { SubjectField } from "./SubjectField";
import type { ValueDraft } from "./valueForm";
import styles from "./knowledge.module.css";

/** A fresh draft for `predicate`, with every field empty. */
export function emptyDraft(predicate: string): ValueDraft {
  return {
    predicate,
    text: "",
    cidr: "",
    criticality: "",
    startsAt: "",
    endsAt: "",
    note: "",
    dependsOn: "",
  };
}

/**
 * The fields that make up the value of a fact of `draft.predicate`. `problem` is the sentence
 * `buildValue` gave, shown under the field it is about.
 */
export function ValueFields({
  draft,
  onChange,
  problem,
}: {
  draft: ValueDraft;
  onChange: (change: Partial<ValueDraft>) => void;
  problem: string | undefined;
}) {
  const { predicate } = draft;
  const label = predicateInfo(predicate).label;
  switch (predicate) {
    case "role":
    case "backup_location":
      return (
        <Field label={label} error={problem}>
          <TextInput
            value={draft.text}
            onChange={(event) => onChange({ text: event.target.value })}
          />
        </Field>
      );
    case "owner_note":
      return (
        <Field label={label} error={problem}>
          <Textarea
            rows={3}
            value={draft.text}
            onChange={(event) => onChange({ text: event.target.value })}
          />
        </Field>
      );
    case "criticality":
      return (
        <Field label={label} error={problem}>
          <SegmentedControl
            label="Criticality"
            value={draft.criticality}
            options={[
              { value: "critical", label: "Critical" },
              { value: "warning", label: "Warning" },
            ]}
            onChange={(criticality) => onChange({ criticality })}
          />
        </Field>
      );
    case "management_path":
    case "trusted_cidr":
      return (
        <Field
          label="Network"
          error={problem}
          hint="For example 192.168.1.0/24. Domovoi stores it in its standard form."
        >
          <TextInput
            mono
            value={draft.cidr}
            autoComplete="off"
            onChange={(event) => onChange({ cidr: event.target.value })}
          />
        </Field>
      );
    case "maintenance_window":
      return (
        <div className={styles.form}>
          <div className={styles.pair}>
            <Field label="Starts (UTC)">
              <TextInput
                type="datetime-local"
                value={draft.startsAt}
                onChange={(event) => onChange({ startsAt: event.target.value })}
              />
            </Field>
            <Field label="Ends (UTC)" error={problem}>
              <TextInput
                type="datetime-local"
                value={draft.endsAt}
                onChange={(event) => onChange({ endsAt: event.target.value })}
              />
            </Field>
          </div>
          <Field label="Note (optional)">
            <TextInput
              value={draft.note}
              onChange={(event) => onChange({ note: event.target.value })}
            />
          </Field>
        </div>
      );
    case "depends_on":
      return (
        <div className={styles.form}>
          <SubjectField
            label="Depends on"
            kinds={predicateInfo(predicate).subjects}
            value={draft.dependsOn}
            onChange={(dependsOn) => onChange({ dependsOn })}
          />
          {problem && <Text muted>{problem}</Text>}
        </div>
      );
    case "never_restart":
      return <Text block>Domovoi will not restart it.</Text>;
    default:
      return null;
  }
}
