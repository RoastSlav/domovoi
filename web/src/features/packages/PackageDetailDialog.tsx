import { useState } from "react";
import { usePackageDetail } from "../../api/hooks/packages";
import {
  Button,
  CopyBlock,
  Dialog,
  KeyValueList,
  QueryBoundary,
  RelativeTime,
  Stack,
  StatusPill,
  Text,
  visibilityAttrs,
} from "../../components";
import { fileStatusMeta, hostPackageStatusMeta } from "../../design/tone";
import { DeletePackageDialog } from "./DeletePackageDialog";
import { signerText } from "./ImportResult";

function formatSize(bytes: number): string {
  return bytes >= 1024 * 1024
    ? `${(bytes / (1024 * 1024)).toFixed(1)} MB`
    : `${Math.max(1, Math.round(bytes / 1024))} kB`;
}

/**
 * One stored package: its digest, signature file, whether its archive is still in the store
 * and which hosts last reported it. The delete button asks for confirmation first.
 */
export function PackageDetailDialog({
  name,
  version,
  onClose,
}: {
  name: string;
  onClose: () => void;
  version: string;
}) {
  const detail = usePackageDetail(name, version, true);
  const [deleting, setDeleting] = useState(false);
  return (
    <>
      <Dialog
        title={`${name} ${version}`}
        onClose={onClose}
        wide
        actions={
          <>
            <Button variant="danger" icon="trash" onClick={() => setDeleting(true)}>
              Delete from the store
            </Button>
            <Button onClick={onClose}>Close</Button>
          </>
        }
      >
        <QueryBoundary query={detail} context="load the package">
          {(pkg) => (
            <Stack gap={4}>
              <KeyValueList
                items={[
                  { label: "Name", value: pkg.name },
                  { label: "Version", value: <Text mono>{pkg.version}</Text> },
                  { label: "Signed by", value: signerText(pkg) },
                  { label: "Imported", value: <RelativeTime at={pkg.imported_at} /> },
                  { label: "Size", value: formatSize(pkg.archive_size) },
                  {
                    label: "Archive",
                    value: (
                      <span {...visibilityAttrs("P0", "packages.store.problem")}>
                        <StatusPill meta={fileStatusMeta(pkg.file_status)} />
                      </span>
                    ),
                  },
                ]}
              />
              <CopyBlock label="Digest" text={pkg.archive_sha256} />
              <CopyBlock label="Signature file" text={pkg.signature_toml} />
              <Stack gap={2}>
                <Text strong block>
                  Installed on
                </Text>
                {pkg.hosts.length === 0 ? (
                  <Text muted block>
                    No host has reported this version.
                  </Text>
                ) : (
                  <ul>
                    {pkg.hosts.map((host) => (
                      <li key={host.host_id}>
                        {host.host_name}{" "}
                        <StatusPill size="sm" meta={hostPackageStatusMeta(host.status)} />{" "}
                        <Text muted size="sm">
                          as last reported <RelativeTime at={host.observed_at} />
                        </Text>
                      </li>
                    ))}
                  </ul>
                )}
              </Stack>
            </Stack>
          )}
        </QueryBoundary>
      </Dialog>
      {deleting && (
        <DeletePackageDialog
          name={name}
          version={version}
          onCancel={() => setDeleting(false)}
          onDeleted={onClose}
        />
      )}
    </>
  );
}
