import type { Editable } from "../../api/types";
import { Stack } from "../../components";
import { Field, ModelField, SecretField } from "./fields";
import { describe, isSecretSet, lockedBy, sourceOf, type FieldSpec } from "./form";
import type { SettingsForm } from "./useSettingsForm";

/** Everything a field needs from the page besides its own spec. */
export interface FieldsContext {
  editable: Editable;
  form: SettingsForm;
  /** Messages the hub gave for each field. */
  issues: Map<string, string[]>;
  schema: Record<string, unknown> | undefined;
}

function FieldFor({ spec, context }: { spec: FieldSpec; context: FieldsContext }) {
  const { editable, form, issues, schema } = context;
  const source = sourceOf(editable.fields, spec.path);
  const common = {
    spec,
    source,
    locked: lockedBy(source),
    hint: spec.hint ?? describe(schema, spec.path),
    errors: [...(form.problems.get(spec.path) ?? []), ...(issues.get(spec.path) ?? [])],
  };
  if (spec.kind === "secret") {
    return (
      <SecretField
        {...common}
        isSet={isSecretSet(editable.values, spec.path)}
        change={form.secret(spec.path)}
        onChange={(change) => form.setSecret(spec.path, change)}
      />
    );
  }
  if (spec.kind === "model") {
    return (
      <ModelField
        {...common}
        value={String(form.value(spec.path))}
        candidate={form.changes}
        slot={spec.slot}
        onChange={(value) => form.setValue(spec.path, value)}
      />
    );
  }
  return (
    <Field
      {...common}
      value={form.value(spec.path)}
      onChange={(value) => form.setValue(spec.path, value)}
    />
  );
}

/** The fields of one section, each as the renderer for its kind. */
export function SectionFields({
  fields,
  context,
}: {
  fields: FieldSpec[];
  context: FieldsContext;
}) {
  return (
    <Stack gap={5}>
      {fields.map((spec) => (
        <FieldFor key={spec.path} spec={spec} context={context} />
      ))}
    </Stack>
  );
}
