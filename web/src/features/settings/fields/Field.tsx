import {
  Cluster,
  Field as KitField,
  InlineMessage,
  Select,
  Stack,
  Switch,
  TextInput,
  Textarea,
  fieldIdForPath,
} from "../../../components";
import { errorText, type Common } from "./common";
import { SourceTag } from "./SourceTag";

/** One plain setting: text, number, list, Discord ids, a choice, a switch or JSON. */
export function Field({
  spec,
  value,
  onChange,
  hint,
  errors,
  locked,
  source,
}: Common & { value: string | boolean; onChange: (value: string | boolean) => void }) {
  const error = errorText(errors);
  const badges = source ? <SourceTag source={source} /> : undefined;
  if (spec.kind === "bool") {
    return (
      <Stack gap={1}>
        <Switch
          id={fieldIdForPath(spec.path)}
          label={spec.label}
          checked={value === true}
          hint={hint}
          disabledReason={locked}
          onChange={onChange}
        />
        {badges && <Cluster gap={2}>{badges}</Cluster>}
        {error && (
          <InlineMessage tone="danger">
            <p>{error}</p>
          </InlineMessage>
        )}
      </Stack>
    );
  }
  let control;
  if (spec.kind === "select") {
    control = (
      <Select value={String(value)} onChange={(event) => onChange(event.target.value)}>
        {(spec.options ?? []).map((option) => (
          <option key={option} value={option}>
            {option || "not set"}
          </option>
        ))}
      </Select>
    );
  } else if (spec.kind === "json") {
    control = (
      <Textarea
        mono
        rows={6}
        value={String(value)}
        spellCheck={false}
        onChange={(event) => onChange(event.target.value)}
      />
    );
  } else {
    control = (
      <TextInput
        inputMode={["number", "id", "ids"].includes(spec.kind) ? "numeric" : undefined}
        mono={["id", "ids"].includes(spec.kind)}
        value={String(value)}
        placeholder={spec.placeholder}
        autoComplete="off"
        onChange={(event) => onChange(event.target.value)}
      />
    );
  }
  return (
    <KitField
      label={spec.label}
      path={spec.path}
      hint={hint}
      error={error}
      locked={locked}
      badges={badges}
    >
      {control}
    </KitField>
  );
}
