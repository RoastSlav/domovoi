import { useState } from "react";
import { useSettingsSchema } from "../../api/hooks/settings";
import type { Editable } from "../../api/types";
import { Button } from "../../components/Button";
import { SectionForm } from "./SectionForm";
import { SECTIONS } from "./sections";
import styles from "./settings.module.css";

/** A new webhook needs a name first; its other fields then appear in its section. */
function AddWebhook({ onAdd }: { onAdd: (name: string) => void }) {
  const [name, setName] = useState("");
  const valid = /^[a-z0-9][a-z0-9_-]{0,31}$/.test(name);
  return (
    <p className={styles.actions}>
      <label htmlFor="new-webhook">New webhook name</label>
      <input id="new-webhook" value={name} onChange={(e) => setName(e.target.value.trim())} />
      <Button disabled={!valid} onClick={() => onAdd(name)}>
        Add webhook
      </Button>
    </p>
  );
}

/** Every editable section. Each saves on its own, against the revision it was loaded at. */
export function EditableSettings({ editable }: { editable: Editable }) {
  const schema = useSettingsSchema();
  const [added, setAdded] = useState<string[]>([]);
  const values = editable.values as Record<string, unknown>;
  // New webhooks get fields but no values, so everything set for them is a change.
  const withAdded = {
    ...values,
    webhooks: {
      ...((values.webhooks ?? {}) as Record<string, unknown>),
      ...Object.fromEntries(added.map((name) => [name, {}])),
    },
  };
  return (
    <>
      {SECTIONS.map((section) => (
        <SectionForm
          key={`${section.title}-${editable.revision}-${added.join(",")}`}
          title={section.title}
          fields={section.fields(section.title === "Webhooks" ? withAdded : values)}
          editable={editable}
          schema={schema.data}
          {...(section.test ? { test: section.test } : {})}
          {...(section.title === "Webhooks"
            ? { intro: <AddWebhook onAdd={(name) => setAdded((a) => [...a, name])} /> }
            : {})}
        />
      ))}
    </>
  );
}
