import { Stack } from "../../components";
import type { Issue } from "../../api/types";
import { Field, ModelField, SecretField } from "../settings/fields";
import { isSecretSet, lockedBy, sourceOf, type FieldSpec } from "../settings/form";
import type { Editable } from "../../api/types";
import type { Wizard } from "./useWizard";

/**
 * The fields of one step. Secrets are write-only, a model field suggests what the provider
 * offers, and a field shows only when the choice it depends on is made.
 */
export function StepFields({
  fields,
  editable,
  wizard,
  issues,
}: {
  editable: Editable;
  fields: FieldSpec[];
  issues: Issue[];
  wizard: Wizard;
}) {
  const values = editable.values as Record<string, unknown>;
  const visible = fields.filter(
    (spec) =>
      !spec.visibleWhen ||
      spec.visibleWhen.values.includes(String(wizard.inputs[spec.visibleWhen.path] ?? "")),
  );
  return (
    <Stack gap={4}>
      {visible.map((spec) => {
        const locked = lockedBy(sourceOf(editable.fields, spec.path));
        const errors = issues.filter((i) => i.path === spec.path).map((i) => i.message);
        if (spec.kind === "secret") {
          return (
            <SecretField
              key={spec.path}
              spec={spec}
              hint={spec.hint}
              locked={locked}
              errors={errors}
              isSet={isSecretSet(values, spec.path)}
              change={wizard.secrets[spec.path] ?? { keep: true }}
              onChange={(change) => wizard.setSecret(spec.path, change)}
            />
          );
        }
        if (spec.kind === "model") {
          return (
            <ModelField
              key={spec.path}
              spec={spec}
              hint={spec.hint}
              locked={locked}
              errors={errors}
              value={String(wizard.inputs[spec.path] ?? "")}
              candidate={wizard.changes}
              onChange={(value) => wizard.setInput(spec.path, value)}
            />
          );
        }
        return (
          <Field
            key={spec.path}
            spec={spec}
            hint={spec.hint}
            locked={locked}
            errors={errors}
            value={wizard.inputs[spec.path] ?? ""}
            onChange={(value) => wizard.setInput(spec.path, value)}
          />
        );
      })}
    </Stack>
  );
}
