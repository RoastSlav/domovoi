import { useId, useState } from "react";
import { ConfirmDialog } from "./ConfirmDialog";
import styles from "./ui.module.css";

/**
 * Asks for the admin's password again before a change that widens trust. The password is
 * passed to `onConfirm` and never kept.
 */
export function PasswordPrompt({
  reason,
  error,
  busy = false,
  onConfirm,
  onCancel,
}: {
  reason: string;
  error?: string | undefined;
  busy?: boolean;
  onConfirm: (password: string) => void;
  onCancel: () => void;
}) {
  const [password, setPassword] = useState("");
  const id = useId();
  return (
    <ConfirmDialog
      title="Confirm with your password"
      confirmLabel="Save"
      confirmDisabled={!password}
      busy={busy}
      onConfirm={() => onConfirm(password)}
      onCancel={onCancel}
    >
      <p>{reason}</p>
      <label htmlFor={id}>Password</label>
      <input
        id={id}
        type="password"
        autoComplete="current-password"
        value={password}
        onChange={(e) => setPassword(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && password) {
            onConfirm(password);
          }
        }}
      />
      {error && (
        <p className={styles.error} role="alert">
          {error}
        </p>
      )}
    </ConfirmDialog>
  );
}
