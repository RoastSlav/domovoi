import { Field, PasswordInput, Stack, Text } from "../../components";
import { HUMAN_ONLY } from "./packageText";

/**
 * The password half of an authorization: the person's own password and the sentence saying
 * why it is asked every time. The value lives only in the dialog that owns this field.
 */
export function PasswordField({
  value,
  onChange,
}: {
  onChange: (value: string) => void;
  value: string;
}) {
  return (
    <Stack gap={3}>
      <Field label="Your password">
        <PasswordInput
          autoComplete="current-password"
          value={value}
          onChange={(event) => onChange(event.target.value)}
        />
      </Field>
      <Text muted block size="sm">
        {HUMAN_ONLY}
      </Text>
    </Stack>
  );
}
