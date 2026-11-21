import type { PackageSummary } from "../../api/types";
import { InlineMessage, KeyValueList, LinkButton, Stack, Text, Tooltip } from "../../components";
import { SIGNATURE_NOTE } from "./packageText";

/** The signer of a package in words: its label, or a note that it has none, with the key id. */
export function signerText(pkg: Pick<PackageSummary, "key_id" | "signer_label">): string {
  return `${pkg.signer_label ?? "a key with no label"} (${pkg.key_id})`;
}

/**
 * What an import did: who signed the package, its digest, that the hub's check is not the
 * hosts' check, and the way on to installing it. Not for failures; the import form shows those.
 */
export function ImportResult({ pkg }: { pkg: PackageSummary }) {
  const title = pkg.already_imported
    ? `${pkg.name} ${pkg.version} is already in the store`
    : `Imported ${pkg.name} ${pkg.version}`;
  const query = `package=${encodeURIComponent(pkg.name)}&version=${encodeURIComponent(pkg.version)}`;
  return (
    <InlineMessage
      tone="success"
      title={title}
      action={<LinkButton to={`/capabilities/install?${query}`}>Install it</LinkButton>}
    >
      <Stack gap={2}>
        <KeyValueList
          items={[
            {
              label: "Signed by",
              value: (
                <>
                  {pkg.signer_label ?? "a key with no label"} <Text mono>{pkg.key_id}</Text>
                </>
              ),
            },
            {
              label: "Digest",
              value: (
                <Tooltip content={pkg.archive_sha256}>
                  <Text mono>{pkg.archive_sha256.slice(0, 12)}</Text>
                </Tooltip>
              ),
            },
          ]}
        />
        <Text size="sm" block>
          {SIGNATURE_NOTE}
        </Text>
      </Stack>
    </InlineMessage>
  );
}
