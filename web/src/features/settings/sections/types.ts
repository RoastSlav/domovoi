import type { ComponentType } from "react";
import type { ProviderSlot, TestTarget } from "../../../api/hooks/settings";
import type { FieldSpec } from "../form";

/** The extra panel a section may draw above its fields. */
export type SectionExtra = "discord-help" | "add-webhook";

/** One section of the settings page. */
export interface SectionSpec {
  /** The URL segment: `/settings/<id>`. */
  id: string;
  title: string;
  /** One sentence under the title saying what the section is for. */
  intro?: string;
  /** The editable fields, from the current values; empty for a section that is only a view. */
  fields: (values: Record<string, unknown>) => FieldSpec[];
  /** A section that draws its own content instead of fields (status, about, account). */
  view?: ComponentType;
  test?: TestTarget;
  /** Which provider a model test targets; the main one by default. */
  testSlot?: ProviderSlot;
  extra?: SectionExtra;
}

/** A section's fields that never depend on the values. */
export const fixed = (fields: FieldSpec[]) => () => fields;
