import { usePackagesStatus } from "../../api/hooks/packages";
import { AllHostsNote } from "../../app/AllHostsNote";
import { KeyValueList, LoadError, Section, Stack, Visible } from "../../components";
import { ImportPackage } from "./ImportPackage";
import { InstalledOnHost } from "./InstalledOnHost";
import { OwnerKeys } from "./OwnerKeys";
import { PackageTable } from "./PackageTable";
import { PackagesStatusNote } from "./PackagesStatusNote";
import { RecentInstalls } from "./RecentInstalls";

function megabytes(bytes: number): string {
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/**
 * The Packages tab of the Capability Center: whether installation is on, importing an
 * owner-signed package, the store, what each host has installed and recent installs. It never
 * starts an install; the install wizard does that after a password and the typed name.
 */
export function PackagesTab() {
  const status = usePackagesStatus();
  const data = status.data;
  const blocked = status.isError
    ? "Could not check whether package installation is on."
    : data === undefined
      ? "Checking whether package installation is on."
      : data.enabled
        ? undefined
        : "Package installation is off on this hub. Turn it on in Settings, Packages.";
  return (
    <Stack gap={6}>
      <AllHostsNote>
        The package store belongs to the hub, so this part ignores the host selector.
      </AllHostsNote>
      {status.isError && (
        <LoadError
          error={status.error}
          context="check package installation"
          onRetry={status.refetch}
        />
      )}
      {data && (
        <Visible category="P0" id="packages.status.note">
          <PackagesStatusNote status={data} />
        </Visible>
      )}
      <Section title="Import a package">
        <ImportPackage blockedReason={blocked} />
      </Section>
      <Section title="Package store">
        <Stack gap={4}>
          <PackageTable installBlocked={blocked} />
          {data && (
            <>
              <Visible category="P2" id="packages.store.usage" label="Store usage">
                <KeyValueList
                  items={[
                    { label: "Packages", value: String(data.artifact_count) },
                    {
                      label: "Space used",
                      value: `${megabytes(data.used_store_bytes)} of ${megabytes(data.max_store_bytes)}`,
                    },
                  ]}
                />
              </Visible>
              <Visible category="P2" id="packages.owner-keys" label="Owner keys">
                <OwnerKeys status={data} />
              </Visible>
            </>
          )}
        </Stack>
      </Section>
      <Section title="Installed on a host">
        <InstalledOnHost />
      </Section>
      <Section title="Recent installs">
        <RecentInstalls />
      </Section>
    </Stack>
  );
}
