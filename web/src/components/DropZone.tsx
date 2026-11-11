import { useId, useRef, useState, type DragEvent } from "react";
import { Icon } from "../design/icons";
import { Button } from "./Button";
import styles from "./dropzone.module.css";

/** Whether `file` matches an `accept` list of extensions (".md") and media types. */
export function acceptsFile(file: File, accept: string | undefined): boolean {
  const rules = (accept ?? "")
    .split(",")
    .map((rule) => rule.trim().toLowerCase())
    .filter(Boolean);
  if (rules.length === 0) {
    return true;
  }
  const name = file.name.toLowerCase();
  const type = file.type.toLowerCase();
  return rules.some((rule) =>
    rule.startsWith(".")
      ? name.endsWith(rule)
      : rule.endsWith("/*")
        ? type.startsWith(rule.slice(0, -1))
        : type === rule,
  );
}

/**
 * A place to drop files or choose them. It is a real file input underneath, so the keyboard
 * and assistive technology use it like any other; dragging files over it only adds a second
 * way in. `onFiles` gets the chosen or dropped files (dropped ones filtered by `accept`).
 *
 * `folderLabel` adds a second button that chooses a whole folder; its files carry their
 * folder-relative paths. Use it for uploads. Do not use it for a single value in a form.
 */
export function DropZone({
  label,
  hint,
  accept,
  multiple = true,
  folderLabel,
  disabledReason,
  onFiles,
}: {
  accept?: string;
  /** Adds a button that chooses a folder; this is its label. */
  folderLabel?: string;
  /** Why files cannot be added now. */
  disabledReason?: string | undefined;
  /** Shown under the label: what is expected. */
  hint?: string;
  /** What the zone says and what its input is called. */
  label: string;
  multiple?: boolean;
  onFiles: (files: File[]) => void;
}) {
  const [dragging, setDragging] = useState(false);
  const folder = useRef<HTMLInputElement>(null);
  const hintId = useId();
  const labelId = useId();
  const disabled = Boolean(disabledReason);

  const over = (event: DragEvent) => {
    event.preventDefault();
    setDragging(!disabled);
  };
  const drop = (event: DragEvent) => {
    event.preventDefault();
    setDragging(false);
    if (!disabled) {
      onFiles([...event.dataTransfer.files].filter((file) => acceptsFile(file, accept)));
    }
  };
  const chosen = (input: HTMLInputElement) => {
    onFiles([...(input.files ?? [])]);
    // The same files can then be chosen again.
    input.value = "";
  };

  return (
    <div className={styles.wrap}>
      <label
        className={styles.zone}
        data-dragging={dragging || undefined}
        data-disabled={disabled || undefined}
        onDragEnter={over}
        onDragOver={over}
        onDragLeave={() => setDragging(false)}
        onDrop={drop}
      >
        <Icon name="plus" size={20} />
        <span className={styles.label} id={labelId}>
          {label}
        </span>
        {hint && (
          <span className={styles.hint} id={hintId}>
            {hint}
          </span>
        )}
        <input
          type="file"
          className={styles.input}
          multiple={multiple}
          {...(accept ? { accept } : {})}
          disabled={disabled}
          aria-labelledby={labelId}
          aria-describedby={hint ? hintId : undefined}
          onChange={(event) => chosen(event.target)}
        />
      </label>
      {folderLabel && (
        <>
          <Button size="sm" disabledReason={disabledReason} onClick={() => folder.current?.click()}>
            {folderLabel}
          </Button>
          <input
            ref={folder}
            type="file"
            className="visually-hidden"
            tabIndex={-1}
            aria-hidden="true"
            multiple
            {...{ webkitdirectory: "" }}
            onChange={(event) => chosen(event.target)}
          />
        </>
      )}
    </div>
  );
}
