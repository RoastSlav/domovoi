import { Field, PasswordInput } from "../../components";

/** The signed-in person's password, asked again for a block. The value stays in the dialog that owns it. */
export function PasswordRow({
  value,
  onChange,
}: {
  onChange: (value: string) => void;
  value: string;
}) {
  return (
    <Field label="Your password" hint="Asked every time. Only a person can do this.">
      <PasswordInput
        autoComplete="current-password"
        value={value}
        onChange={(event) => onChange(event.target.value)}
      />
    </Field>
  );
}
