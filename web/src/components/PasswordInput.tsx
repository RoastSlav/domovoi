import { useState, type InputHTMLAttributes, type KeyboardEvent } from "react";
import { Icon } from "../design/icons";
import { IconButton } from "./IconButton";
import { TextInput } from "./inputs";
import styles from "./auth.module.css";

/**
 * A password input with a button that shows or hides what was typed, and a note ("Caps
 * Lock is on") while the Caps Lock key is down. The note sits inside the input, so nothing
 * around it moves. Put it inside a `Field`.
 *
 * `subject` names the value in the button's label ("Show password"); give it when a form
 * has two ("repeated password"). Use it for typing a new or existing password. Do not use it
 * for tokens or keys that are only pasted.
 */
export function PasswordInput({
  subject = "password",
  onKeyDown,
  onKeyUp,
  onBlur,
  ...props
}: Omit<InputHTMLAttributes<HTMLInputElement>, "title" | "type"> & { subject?: string }) {
  const [shown, setShown] = useState(false);
  const [caps, setCaps] = useState(false);
  const track = (event: KeyboardEvent<HTMLInputElement>) =>
    setCaps(event.getModifierState("CapsLock"));
  return (
    <div className={styles.passwordWrap} data-caps={caps || undefined}>
      <TextInput
        {...props}
        className={styles.passwordInput}
        type={shown ? "text" : "password"}
        onKeyDown={(event) => {
          track(event);
          onKeyDown?.(event);
        }}
        onKeyUp={(event) => {
          track(event);
          onKeyUp?.(event);
        }}
        onBlur={(event) => {
          setCaps(false);
          onBlur?.(event);
        }}
      />
      <span className={styles.caps} role="status">
        {caps && (
          <>
            <Icon name="alert" size={14} />
            Caps Lock is on
          </>
        )}
      </span>
      <span className={styles.toggle}>
        <IconButton
          icon={shown ? "eye-off" : "eye"}
          label={shown ? `Hide ${subject}` : `Show ${subject}`}
          size="sm"
          onClick={() => setShown((now) => !now)}
        />
      </span>
    </div>
  );
}
