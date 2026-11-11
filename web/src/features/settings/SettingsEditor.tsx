import { useState } from "react";
import type { SaveRequest } from "../../api/hooks/settings";
import { useSession } from "../../api/hooks/session";
import type { Editable } from "../../api/types";
import { Stack } from "../../components";
import { EditorForm } from "./EditorForm";
import { RestartBanner } from "./RestartBanner";
import { useApplySettings } from "./useApply";

/**
 * The editable settings and what happens after a save. The restart is told here, above the
 * form, because the form is rebuilt when the saved revision arrives and must not take the
 * message with it.
 */
export function SettingsEditor({ editable }: { editable: Editable }) {
  // Watched so the restart flow can wait for the session to answer again before "Back online".
  useSession();
  const apply = useApplySettings();
  const [saved, setSaved] = useState(0);
  const save = (request: SaveRequest, count: number) => {
    setSaved(count);
    return apply.run(request);
  };
  return (
    <Stack gap={5}>
      <RestartBanner phase={apply.phase} changes={saved} onDismiss={apply.dismiss} />
      <EditorForm
        key={editable.revision}
        editable={editable}
        saving={apply.pending}
        onSave={save}
      />
    </Stack>
  );
}
