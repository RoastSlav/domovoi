import { useMemo, useState } from "react";
import type { Changes, SecretChange } from "../../api/hooks/settings";
import type { Editable } from "../../api/types";
import { buildChanges, initialInputs, inputProblem, type FieldSpec } from "./form";
import { SECTIONS } from "./sections";

type Input = string | boolean;

/** The whole settings form: every section's inputs, kept while the person moves between them. */
export interface SettingsForm {
  /** Every field of every section, hidden ones included; what a save is built from. */
  all: FieldSpec[];
  /** The fields a section shows now, after `visibleWhen`. */
  visible: (sectionId: string) => FieldSpec[];
  /** The visible field paths of every section, in page order. */
  visiblePaths: Map<string, string[]>;
  value: (path: string) => Input;
  setValue: (path: string, value: Input) => void;
  secret: (path: string) => SecretChange;
  setSecret: (path: string, change: SecretChange) => void;
  /** What a save would send. */
  changes: () => Changes;
  /** Unsaved changes in all sections, and in one. */
  count: number;
  countIn: (sectionId: string) => number;
  /** Why a visible input cannot be saved, by field path. */
  problems: Map<string, string[]>;
  addWebhook: (name: string) => void;
  discard: () => void;
}

/**
 * Holds the edits of every section in one place, so switching sections loses nothing and
 * one Save sends everything. The caller remounts it (a `key`) when the saved revision changes.
 */
export function useSettingsForm(editable: Editable): SettingsForm {
  const [edits, setEdits] = useState<Record<string, Input>>({});
  const [secrets, setSecrets] = useState<Record<string, SecretChange>>({});
  const [added, setAdded] = useState<string[]>([]);

  const values = useMemo(() => {
    const base = editable.values as Record<string, unknown>;
    // New webhooks get fields but no values, so everything set for them is a change.
    const hooks = (base.webhooks ?? {}) as Record<string, unknown>;
    return {
      ...base,
      webhooks: { ...hooks, ...Object.fromEntries(added.map((name) => [name, {}])) },
    };
  }, [editable.values, added]);
  const bySection = useMemo(
    () => new Map(SECTIONS.map((section) => [section.id, section.fields(values)])),
    [values],
  );
  const all = useMemo(() => [...bySection.values()].flat(), [bySection]);
  const initial = useMemo(() => initialInputs(all, values), [all, values]);
  const inputs = { ...initial, ...edits };

  const visibleIn = (id: string) =>
    (bySection.get(id) ?? []).filter(
      (field) =>
        !field.visibleWhen ||
        field.visibleWhen.values.includes(String(inputs[field.visibleWhen.path] ?? "")),
    );
  const visiblePaths = new Map(
    [...bySection.keys()].map((id) => [id, visibleIn(id).map((field) => field.path)]),
  );

  const dirty = (field: FieldSpec): boolean => {
    if (field.kind === "secret") {
      const change = secrets[field.path];
      return change !== undefined && !("keep" in change);
    }
    return field.path in edits && edits[field.path] !== initial[field.path];
  };
  const countIn = (id: string) => (bySection.get(id) ?? []).filter(dirty).length;

  const problems = new Map<string, string[]>();
  for (const [id] of bySection) {
    for (const field of visibleIn(id)) {
      const input = inputs[field.path];
      const bad = input === undefined ? undefined : inputProblem(input, field.kind);
      if (bad) {
        problems.set(field.path, [bad]);
      }
    }
  }

  return {
    all,
    visible: visibleIn,
    visiblePaths,
    value: (path) => inputs[path] ?? "",
    setValue: (path, value) => setEdits((now) => ({ ...now, [path]: value })),
    secret: (path) => secrets[path] ?? { keep: true },
    setSecret: (path, change) => setSecrets((now) => ({ ...now, [path]: change })),
    changes: () => buildChanges(all, initial, inputs, secrets),
    count: all.filter(dirty).length,
    countIn,
    problems,
    addWebhook: (name) => setAdded((now) => (now.includes(name) ? now : [...now, name])),
    discard: () => {
      setEdits({});
      setSecrets({});
      setAdded([]);
    },
  };
}
