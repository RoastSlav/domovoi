import { useId, useState } from "react";
import type { SecretChange } from "../../api/hooks/settings";
import { Button } from "../../components/Button";
import type { FieldSpec } from "./form";
import styles from "./settings.module.css";

interface Common {
  spec: FieldSpec;
  /** Help text. */
  hint?: string | undefined;
  /** Problems with the value, from the hub or the input. */
  errors?: string[] | undefined;
  /** When set, the field is read-only and this says why. */
  locked?: string | undefined;
}

function Messages({ id, hint, errors, locked }: { id: string } & Omit<Common, "spec">) {
  return (
    <>
      {locked && (
        <p id={`${id}-locked`} className={styles.locked}>
          {locked}
        </p>
      )}
      {hint && (
        <p id={`${id}-hint`} className={styles.hint}>
          {hint}
        </p>
      )}
      {errors?.map((error) => (
        <p key={error} className={styles.fieldError} role="alert">
          {error}
        </p>
      ))}
    </>
  );
}

function describedBy(id: string, { hint, locked }: Omit<Common, "spec">): string | undefined {
  const ids = [locked && `${id}-locked`, hint && `${id}-hint`].filter(Boolean);
  return ids.length ? ids.join(" ") : undefined;
}

/** One plain setting: text, number, list, Discord ids, a choice, a switch or JSON. */
export function Field({
  spec,
  value,
  onChange,
  ...common
}: Common & { value: string | boolean; onChange: (value: string | boolean) => void }) {
  const id = useId();
  const readOnly = Boolean(common.locked);
  const invalid = Boolean(common.errors?.length);
  const aria = {
    "aria-describedby": describedBy(id, common),
    "aria-invalid": invalid || undefined,
  };
  let control;
  if (spec.kind === "bool") {
    control = (
      <input
        id={id}
        type="checkbox"
        checked={value === true}
        disabled={readOnly}
        onChange={(e) => onChange(e.target.checked)}
        {...aria}
      />
    );
  } else if (spec.kind === "select") {
    control = (
      <select
        id={id}
        value={String(value)}
        disabled={readOnly}
        onChange={(e) => onChange(e.target.value)}
        {...aria}
      >
        {(spec.options ?? []).map((option) => (
          <option key={option} value={option}>
            {option || "not set"}
          </option>
        ))}
      </select>
    );
  } else if (spec.kind === "json") {
    control = (
      <textarea
        id={id}
        rows={6}
        value={String(value)}
        readOnly={readOnly}
        spellCheck={false}
        onChange={(e) => onChange(e.target.value)}
        {...aria}
      />
    );
  } else {
    control = (
      <input
        id={id}
        type="text"
        inputMode={spec.kind === "number" || spec.kind === "ids" ? "numeric" : undefined}
        value={String(value)}
        placeholder={spec.placeholder}
        readOnly={readOnly}
        autoComplete="off"
        onChange={(e) => onChange(e.target.value)}
        {...aria}
      />
    );
  }
  return (
    <div className={styles.field}>
      <label htmlFor={id}>{spec.label}</label>
      {control}
      <Messages id={id} {...common} />
    </div>
  );
}

/**
 * A write-only secret: it shows whether a value is set, never the value. Replace types a
 * new one; Remove clears it. Left alone, it is kept.
 */
export function SecretField({
  spec,
  isSet,
  change,
  onChange,
  ...common
}: Common & {
  isSet: boolean;
  change: SecretChange;
  onChange: (change: SecretChange) => void;
}) {
  const id = useId();
  const [replacing, setReplacing] = useState(false);
  const readOnly = Boolean(common.locked);
  const cleared = "clear" in change;
  return (
    <div className={styles.field}>
      <span id={`${id}-label`} className={styles.label}>
        {spec.label}
      </span>
      <p className={styles.secretState} aria-labelledby={`${id}-label`}>
        {cleared ? "Will be removed" : isSet ? "Set" : "Not set"}
      </p>
      {replacing ? (
        <span className={styles.secretRow}>
          <label htmlFor={id} className="visually-hidden">
            New {spec.label}
          </label>
          <input
            id={id}
            type="password"
            autoComplete="new-password"
            value={"set" in change ? change.set : ""}
            onChange={(e) => onChange(e.target.value ? { set: e.target.value } : { keep: true })}
            aria-describedby={describedBy(id, common)}
          />
          <Button
            onClick={() => {
              setReplacing(false);
              onChange({ keep: true });
            }}
          >
            Cancel
          </Button>
        </span>
      ) : (
        !readOnly && (
          <span className={styles.secretRow}>
            {cleared ? (
              <Button onClick={() => onChange({ keep: true })}>Undo</Button>
            ) : (
              <>
                <Button onClick={() => setReplacing(true)}>
                  {isSet ? `Replace ${spec.label}` : `Set ${spec.label}`}
                </Button>
                {isSet && <Button onClick={() => onChange({ clear: true })}>Remove</Button>}
              </>
            )}
          </span>
        )
      )}
      <Messages id={id} {...common} />
    </div>
  );
}
