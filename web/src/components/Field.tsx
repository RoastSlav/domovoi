import { createContext, useContext, useId, type ReactNode } from "react";
import { Icon } from "../design/icons";
import styles from "./form.module.css";

/** What a `Field` tells the input inside it. */
export interface FieldWiring {
  "aria-describedby"?: string | undefined;
  "aria-invalid"?: true | undefined;
  id?: string | undefined;
  /** The field is locked: inputs inside it show their value but cannot change it. */
  readOnly?: true | undefined;
  required?: boolean | undefined;
}

const FieldContext = createContext<FieldWiring>({});

/** The id and ARIA attributes of the enclosing `Field`; kit inputs spread this on themselves. */
export function useFieldWiring(): FieldWiring {
  return useContext(FieldContext);
}

/** The id of the field that edits the value at a hub issue path, like `llm.primary.model`. */
export function fieldIdForPath(path: string): string {
  return `field-${path.replace(/[^\w-]/g, "-")}`;
}

/**
 * A labelled form control: the label, the input (a kit input as the child), and one line
 * under it that is the hint, or the error while the value is invalid. It wires the label,
 * `aria-describedby` and `aria-invalid` to the input.
 *
 * Use it around every input. `path` is the hub's settings path for the value, so
 * `FormErrorSummary` can link an issue to this field. `locked` makes the input read-only and
 * says why; `badges` sit beside the label (where a setting's value comes from, for example).
 */
export function Field({
  label,
  hint,
  error,
  required,
  id,
  path,
  locked,
  badges,
  children,
}: {
  /** Small tags beside the label. */
  badges?: ReactNode;
  children: ReactNode;
  /** What is wrong with the value; replaces the hint. */
  error?: ReactNode;
  /** One line saying what the input expects. */
  hint?: ReactNode;
  id?: string | undefined;
  label: ReactNode;
  /** Why the value cannot be changed here; the input becomes read-only. */
  locked?: ReactNode;
  path?: string;
  required?: boolean;
}) {
  const generated = useId();
  const inputId = id ?? (path ? fieldIdForPath(path) : generated);
  const noteId = `${inputId}-note`;
  const lockedId = `${inputId}-locked`;
  const invalid = Boolean(error);
  const described = [locked && lockedId, (error || hint) && noteId].filter(Boolean).join(" ");
  const wiring: FieldWiring = {
    id: inputId,
    required,
    readOnly: locked ? true : undefined,
    "aria-invalid": invalid || undefined,
    "aria-describedby": described || undefined,
  };
  return (
    <div className={styles.field}>
      <div className={styles.labelRow}>
        <label className={styles.label} htmlFor={inputId}>
          {label}
          {required && <span className={styles.required}>(required)</span>}
        </label>
        {badges}
      </div>
      <FieldContext.Provider value={wiring}>{children}</FieldContext.Provider>
      {locked && (
        <p className={styles.hint} id={lockedId}>
          {locked}
        </p>
      )}
      {error ? (
        <p className={styles.error} id={noteId} role="alert">
          <Icon name="alert-circle" size={14} />
          <span>{error}</span>
        </p>
      ) : (
        hint && (
          <p className={styles.hint} id={noteId}>
            {hint}
          </p>
        )
      )}
    </div>
  );
}
