import { useState } from "react";
import { ConfirmDialog } from "./ConfirmDialog";
import { Field } from "./Field";
import { TextInput } from "./inputs";

/**
 * Asks for the admin's password again before a change that widens trust. The password is
 * passed to `onConfirm` and never kept.
 *
 * Use it wherever the hub demands password re-entry. `error` is the failure in words, shown
 * under the field.
 */
export function PasswordPrompt({
  reason,
  error,
  busy = false,
  confirmLabel = "Save",
  onConfirm,
  onCancel,
}: {
  busy?: boolean;
  confirmLabel?: string;
  error?: string | undefined;
  onCancel: () => void;
  onConfirm: (password: string) => void;
  reason: string;
}) {
  const [password, setPassword] = useState("");
  return (
    <ConfirmDialog
      title="Confirm with your password"
      consequence={reason}
      confirmLabel={confirmLabel}
      confirmDisabledReason={password ? undefined : "Enter your password first."}
      busy={busy}
      onConfirm={() => onConfirm(password)}
      onCancel={onCancel}
    >
      <Field label="Password" error={error}>
        <TextInput
          type="password"
          autoComplete="current-password"
          value={password}
          onChange={(event) => setPassword(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && password) {
              onConfirm(password);
            }
          }}
        />
      </Field>
    </ConfirmDialog>
  );
}
