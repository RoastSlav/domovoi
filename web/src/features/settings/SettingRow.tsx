import { useId, type ReactNode } from "react";
import { fieldIdForPath } from "../../components";
import { Icon } from "../../design/icons";
import styles from "./row.module.css";

/** What a row tells the control inside it: its id and the ARIA that ties it to the text. */
export interface RowWiring {
  "aria-describedby"?: string | undefined;
  "aria-invalid"?: true | undefined;
  id: string;
}

/** A bordered card that holds the rows of one settings section. */
export function SettingsCard({ children }: { children: ReactNode }) {
  return <div className={styles.card}>{children}</div>;
}

/** The name of a group of rows inside a card (one webhook, for example). */
export function RowHeading({ children }: { children: ReactNode }) {
  return <h3 className={styles.heading}>{children}</h3>;
}

/** The words of a row: label, tags beside it, then the locked reason and the hint or error. */
function RowText({
  label,
  labelFor,
  badges,
  hint,
  error,
  locked,
  ids,
}: {
  badges?: ReactNode;
  error?: ReactNode;
  hint?: ReactNode;
  ids: { locked: string; note: string };
  label: ReactNode;
  /** The id of the control the label names; without it the label is plain text. */
  labelFor?: string | undefined;
  locked?: ReactNode;
}) {
  return (
    <div className={styles.text}>
      <div className={styles.labelLine}>
        {labelFor ? (
          <label className={styles.label} htmlFor={labelFor}>
            {label}
          </label>
        ) : (
          <span className={styles.label}>{label}</span>
        )}
        {badges}
      </div>
      {locked && (
        <p className={styles.hint} id={ids.locked}>
          {locked}
        </p>
      )}
      {error ? (
        <p className={styles.error} id={ids.note} role="alert">
          <Icon name="alert-circle" size={14} />
          <span>{error}</span>
        </p>
      ) : (
        hint && (
          <p className={styles.hint} id={ids.note}>
            {hint}
          </p>
        )
      )}
    </div>
  );
}

/**
 * One setting as a row of a settings card: the text on the left, the control from
 * `children` on the right (under the text when `wide`). It wires the label, the hint and
 * the error to the control, which spreads the wiring it is given.
 *
 * `path` is the hub's settings path, so a problem summary can link to the control; `id` is
 * for a control that has no path (the new-webhook name).
 */
export function SettingRow({
  label,
  path,
  id,
  badges,
  hint,
  error,
  locked,
  wide = false,
  plainLabel = false,
  children,
}: {
  badges?: ReactNode;
  children: (wiring: RowWiring) => ReactNode;
  error?: ReactNode;
  hint?: ReactNode;
  id?: string;
  label: ReactNode;
  /** Why the value cannot be changed here. */
  locked?: ReactNode;
  path?: string;
  /** The label is text, not a `label` element: the control is a button, not an input. */
  plainLabel?: boolean;
  /** The control goes under the text at full width. */
  wide?: boolean;
}) {
  const generated = useId();
  const controlId = id ?? (path ? fieldIdForPath(path) : generated);
  const ids = { locked: `${controlId}-locked`, note: `${controlId}-note` };
  const described = [locked && ids.locked, (error || hint) && ids.note].filter(Boolean).join(" ");
  return (
    <div className={styles.row} data-wide={wide || undefined}>
      <RowText
        label={label}
        labelFor={plainLabel ? undefined : controlId}
        badges={badges}
        hint={hint}
        error={error}
        locked={locked}
        ids={ids}
      />
      <div className={styles.control}>
        {children({
          id: controlId,
          "aria-describedby": described || undefined,
          "aria-invalid": error ? true : undefined,
        })}
      </div>
    </div>
  );
}

/** A row whose control is the kit `Switch`, which carries its own label, tags and hint. */
export function SwitchRow({ children, error }: { children: ReactNode; error?: ReactNode }) {
  return (
    <div className={styles.row} data-switch>
      {children}
      {error && (
        <p className={styles.error} role="alert">
          <Icon name="alert-circle" size={14} />
          <span>{error}</span>
        </p>
      )}
    </div>
  );
}

/** A row that only shows a value: the read-only settings page and summaries. */
export function ValueRow({ label, children }: { children: ReactNode; label: string }) {
  return (
    <div className={styles.row}>
      <RowText label={label} ids={{ locked: "", note: "" }} />
      <div className={styles.value}>{children}</div>
    </div>
  );
}
