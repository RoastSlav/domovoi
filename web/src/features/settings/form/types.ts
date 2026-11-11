import type { ProviderSlot } from "../../../api/hooks/settings";

/** How a field is edited. */
export type FieldKind =
  "text" | "number" | "bool" | "select" | "list" | "id" | "ids" | "secret" | "model" | "json";

/** One editable setting. */
export interface FieldSpec {
  /** Dotted path in `hub.toml`, for example `notify.digest.time`. */
  path: string;
  label: string;
  kind: FieldKind;
  /** For `select`: the choices; an empty string means "not set". */
  options?: string[];
  /** Overrides the help text from the schema. */
  hint?: string;
  placeholder?: string;
  /** Show this field only when another input has one of these values. */
  visibleWhen?: { path: string; values: string[] };
  /** For `model`: which provider lists the suggestions; the main one by default. */
  slot?: ProviderSlot;
}
