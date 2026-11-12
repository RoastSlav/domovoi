import { useState } from "react";
import { Button, Cluster, TextInput } from "../../components";
import { SettingRow, SettingsCard } from "./SettingRow";

/** A new webhook needs a name first; its other fields then appear in its section. */
export function AddWebhook({ onAdd }: { onAdd: (name: string) => void }) {
  const [name, setName] = useState("");
  const valid = /^[a-z0-9][a-z0-9_-]{0,31}$/.test(name);
  const add = () => {
    onAdd(name);
    setName("");
  };
  return (
    <SettingsCard>
      <SettingRow
        label="New webhook name"
        hint="Lower-case letters, digits, dashes and underscores. It becomes the address /hooks/<name>."
      >
        {(wiring) => (
          <Cluster wrap={false}>
            <TextInput
              {...wiring}
              value={name}
              mono
              autoComplete="off"
              onChange={(event) => setName(event.target.value.trim())}
            />
            <Button
              onClick={add}
              disabledReason={
                valid
                  ? undefined
                  : "Enter a name of lower-case letters, digits, dashes or underscores."
              }
            >
              Add webhook
            </Button>
          </Cluster>
        )}
      </SettingRow>
    </SettingsCard>
  );
}
