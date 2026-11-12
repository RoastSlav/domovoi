import { Select, Switch, TextInput, Textarea, fieldIdForPath } from "../../../components";
import { SettingRow, SwitchRow } from "../SettingRow";
import { errorText, type Common } from "./common";
import { SourceTag } from "./SourceTag";

/** One plain setting: text, number, list, Discord ids, a choice, a switch or JSON, as a row. */
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
      <SwitchRow error={error}>
        <Switch
          id={fieldIdForPath(spec.path)}
          label={spec.label}
          badges={badges}
          hint={hint}
          checked={value === true}
          disabledReason={locked}
          onChange={onChange}
        />
      </SwitchRow>
    );
  }
  return (
    <SettingRow
      label={spec.label}
      path={spec.path}
      hint={hint}
      error={error}
      locked={locked}
      badges={badges}
      wide={spec.kind === "json"}
    >
      {(wiring) => {
        if (spec.kind === "select") {
          return (
            <Select
              {...wiring}
              // The row's locked line says why a select cannot be changed.
              // eslint-disable-next-line domovoi/disabled-needs-reason
              disabled={Boolean(locked)}
              value={String(value)}
              onChange={(event) => onChange(event.target.value)}
            >
              {(spec.options ?? []).map((option) => (
                <option key={option} value={option}>
                  {option || "not set"}
                </option>
              ))}
            </Select>
          );
        }
        if (spec.kind === "json") {
          return (
            <Textarea
              {...wiring}
              mono
              rows={6}
              readOnly={Boolean(locked)}
              value={String(value)}
              spellCheck={false}
              onChange={(event) => onChange(event.target.value)}
            />
          );
        }
        return (
          <TextInput
            {...wiring}
            inputMode={["number", "id", "ids"].includes(spec.kind) ? "numeric" : undefined}
            mono={["id", "ids"].includes(spec.kind)}
            readOnly={Boolean(locked)}
            value={String(value)}
            placeholder={spec.placeholder}
            autoComplete="off"
            onChange={(event) => onChange(event.target.value)}
          />
        );
      }}
    </SettingRow>
  );
}
