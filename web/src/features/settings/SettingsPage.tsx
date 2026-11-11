import { useSettings } from "../../api/hooks/system";
import { PageHeader, QueryBoundary, Skeleton, Stack } from "../../components";
import { AllHostsNote } from "../../app/AllHostsNote";
import { ReadOnlySettings } from "./ReadOnlySettings";
import { SettingsEditor } from "./SettingsEditor";

const INTRO_EDITABLE =
  "How this hub runs. Fields set in hub.toml or the environment are read-only here.";

/**
 * The settings: sections you can edit when the hub manages them in the UI (each field shows
 * where its value comes from), or everything read-only when a file manages them. Secrets
 * show only as set or not set.
 */
export function SettingsPage() {
  const settings = useSettings();
  return (
    <Stack gap={5}>
      <PageHeader
        title="Settings"
        docs="install/config-as-code.html"
        intro={settings.data?.editable ? INTRO_EDITABLE : "How this hub runs."}
      />
      <AllHostsNote>Settings belong to the hub, not to one host.</AllHostsNote>
      <QueryBoundary
        query={settings}
        context="load settings"
        loading={<Skeleton shape="rows" count={5} />}
      >
        {(data) =>
          data.editable ? (
            <SettingsEditor editable={data.editable} />
          ) : (
            <ReadOnlySettings settings={data} />
          )
        }
      </QueryBoundary>
    </Stack>
  );
}
