import type { AuditVerification } from "../../api/types";
import { CopyButton, KeyValueList, Tooltip } from "../../components";

function Hash({ value }: { value: string }) {
  return (
    <>
      <Tooltip content={value}>
        <span tabIndex={0}>{value.slice(0, 12)}</span>
      </Tooltip>{" "}
      <CopyButton text={value} />
    </>
  );
}

/** The anchor the legacy history is judged against, as a list of facts. */
export function AnchorDetails({ anchor }: { anchor: AuditVerification["anchor"] }) {
  return (
    <KeyValueList
      mono
      items={[
        { label: "Install ID", value: anchor.install_id },
        { label: "Entries before the chain", value: String(anchor.legacy_count) },
        {
          label: "Last of them",
          value: anchor.legacy_last_id === 0 ? "–" : `#${anchor.legacy_last_id}`,
        },
        { label: "Digest", value: <Hash value={anchor.legacy_digest} /> },
        { label: "Anchor hash", value: <Hash value={anchor.anchor_hash} /> },
      ]}
    />
  );
}
