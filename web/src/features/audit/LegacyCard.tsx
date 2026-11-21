import type { AuditVerification } from "../../api/types";
import { Card, StatusPill } from "../../components";
import { legacyStatusMeta } from "../../design/tone";
import styles from "./audit.module.css";

function sentence(check: AuditVerification): string {
  const { legacy, anchor } = check;
  if (anchor.legacy_count === 0) {
    return "This install has no entries from before the chain.";
  }
  switch (legacy.status) {
    case "verified":
      return `${legacy.count} entries from before the chain. Their digest matches.`;
    case "count_only":
      return `${legacy.count} entries from before the chain. Their digest was not compared in this check.`;
    case "mismatch":
      return `Expected ${legacy.expected_count} entries and found ${legacy.actual_count}. The digest ${legacy.digest_matches ? "matches" : "differs"}. The check cannot say which entry changed.`;
    case "not_verifiable":
      return "Older entries were pruned, so they cannot be checked.";
    default:
      return "";
  }
}

/** The legacy history's status: entries written before the chain, checked as a count and a digest. */
export function LegacyCard({ check }: { check: AuditVerification }) {
  return (
    <Card>
      <div className={styles.cardBody}>
        <h3>Legacy history</h3>
        <div>
          <StatusPill meta={legacyStatusMeta(check.legacy.status)} />
        </div>
        <p>{sentence(check)}</p>
      </div>
    </Card>
  );
}
