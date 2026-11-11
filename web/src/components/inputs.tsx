import type {
  InputHTMLAttributes,
  ReactNode,
  Ref,
  SelectHTMLAttributes,
  TextareaHTMLAttributes,
} from "react";
import { useFieldWiring } from "./Field";
import styles from "./form.module.css";

function classes(...names: (string | false | undefined)[]): string {
  return names.filter(Boolean).join(" ");
}

/**
 * A one-line text input (also password, email, search, url). Put it inside a `Field`.
 * `mono` is for commands, paths and IDs.
 */
export function TextInput({
  mono = false,
  className,
  type = "text",
  ...props
}: Omit<InputHTMLAttributes<HTMLInputElement>, "title"> & {
  mono?: boolean;
  ref?: Ref<HTMLInputElement>;
}) {
  return (
    <input
      type={type}
      className={classes(styles.input, mono && styles.mono, className)}
      {...useFieldWiring()}
      {...props}
    />
  );
}

/** A number input with tabular digits. Put it inside a `Field`; say the range in its hint. */
export function NumberInput({
  className,
  ...props
}: Omit<InputHTMLAttributes<HTMLInputElement>, "title" | "type">) {
  return (
    <input
      type="number"
      inputMode="numeric"
      className={classes(styles.input, styles.number, className)}
      {...useFieldWiring()}
      {...props}
    />
  );
}

/** A multi-line text input. Put it inside a `Field`. `mono` is for scripts and logs. */
export function Textarea({
  mono = false,
  className,
  ...props
}: Omit<TextareaHTMLAttributes<HTMLTextAreaElement>, "title"> & {
  mono?: boolean;
  ref?: Ref<HTMLTextAreaElement>;
}) {
  return (
    <textarea
      className={classes(styles.input, styles.textarea, mono && styles.mono, className)}
      {...useFieldWiring()}
      {...props}
    />
  );
}

/** A native select: `<option>` elements as children. Put it inside a `Field`. */
export function Select({
  className,
  children,
  ...props
}: Omit<SelectHTMLAttributes<HTMLSelectElement>, "title"> & { children: ReactNode }) {
  // A select has no read-only state, so a locked field disables it; the reason stays in text.
  const { readOnly, ...wiring } = useFieldWiring();
  return (
    <select
      className={classes(styles.input, className)}
      {...wiring}
      {...props}
      disabled={props.disabled || readOnly}
    >
      {children}
    </select>
  );
}

interface ToggleProps {
  checked: boolean;
  /** The id of the input, for a link that must focus it. */
  id?: string | undefined;
  /** Why it cannot be changed now. */
  disabledReason?: string | undefined;
  /** One line under the label. */
  hint?: ReactNode;
  label: ReactNode;
  onChange: (checked: boolean) => void;
}

function Toggle({
  id,
  checked,
  onChange,
  label,
  hint,
  disabledReason,
  switchRole,
}: ToggleProps & { switchRole: boolean }) {
  return (
    <label className={styles.check} data-disabled={disabledReason ? true : undefined}>
      <input
        id={id}
        type="checkbox"
        role={switchRole ? "switch" : undefined}
        className={switchRole ? styles.switch : undefined}
        checked={checked}
        aria-disabled={disabledReason ? true : undefined}
        onChange={(event) => {
          if (!disabledReason) {
            onChange(event.target.checked);
          }
        }}
      />
      <span className={styles.checkText}>
        <span>{label}</span>
        {(disabledReason ?? hint) && <span className={styles.hint}>{disabledReason ?? hint}</span>}
      </span>
    </label>
  );
}

/**
 * A checkbox with its label: one of several independent choices, saved with the form.
 * Use `Switch` for a setting that takes effect by itself.
 */
export function Checkbox(props: ToggleProps) {
  return <Toggle {...props} switchRole={false} />;
}

/**
 * An on/off switch with its label, for a setting that is simply on or off ("Enable the
 * check"). Green when on. Use `Checkbox` inside a list of choices.
 */
export function Switch(props: ToggleProps) {
  return <Toggle {...props} switchRole />;
}
