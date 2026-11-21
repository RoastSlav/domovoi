import type { AuditVerification } from "../../api/types";
import { Card, CopyBlock, InlineMessage, StatusPill } from "../../components";
import { breakReasonText } from "../../design/auditText";
import { chainStatusMeta } from "../../design/tone";
import styles from "./audit.module.css";

/** The chained history's status, what was checked and the head hash to keep off the host. */
export function ChainCard({ check }: { check: AuditVerification }) {
  const { chained, head } = check;
  return (
    <Card>
      <div className={styles.cardBody}>
        <h3>Chained history</h3>
        <div>
          <StatusPill meta={chainStatusMeta(chained.status)} />
        </div>
        {chained.status === "intact" && (
          <p>
            {chained.checked} entries checked, up to #{chained.head_seq}.
          </p>
        )}
        {chained.status === "empty" && <p>No entries since the chain began.</p>}
        {chained.status === "broken" && (
          <InlineMessage tone="danger" title={`Broken at entry #${chained.first_broken_seq}`}>
            {breakReasonText(chained.break_reason ?? "", chained.break_text ?? null)}{" "}
            {chained.checked} entries were accepted before it.
          </InlineMessage>
        )}
        {head ? (
          <>
            <CopyBlock label="Head hash" text={head.hash} />
            <p className={styles.muted}>
              Entry #{head.seq}. Keep a copy somewhere off this host: it is the only way to notice
              that the newest entries were removed.
            </p>
          </>
        ) : (
          <p className={styles.muted}>There is no head hash until the first chained entry.</p>
        )}
      </div>
    </Card>
  );
}
