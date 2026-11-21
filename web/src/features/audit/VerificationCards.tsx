import type { UseQueryResult } from "@tanstack/react-query";
import type { AuditVerification } from "../../api/types";
import { Button, Grid, LoadError, Progress, RelativeTime, Stack } from "../../components";
import { scopeSentence } from "../../design/auditText";
import { ChainCard } from "./ChainCard";
import { LegacyCard } from "./LegacyCard";
import styles from "./audit.module.css";

/** Both histories' status cards, the line saying what was checked, and the whole-log check. */
export function VerificationCards({
  recent,
  full,
  fullAsked,
  onCheckFull,
}: {
  full: UseQueryResult<AuditVerification>;
  fullAsked: boolean;
  onCheckFull: () => void;
  recent: AuditVerification;
}) {
  const check = full.data ?? recent;
  const running = fullAsked && full.isFetching;
  return (
    <Stack gap={3}>
      {fullAsked && full.error && !full.data && (
        <LoadError error={full.error} context="check the whole log" onRetry={full.refetch} />
      )}
      <Grid min="lg">
        <ChainCard check={check} />
        <LegacyCard check={check} />
      </Grid>
      {running && <Progress label="Checking every entry…" />}
      <div className={styles.footer}>
        <span className={styles.muted}>
          Checked <RelativeTime at={check.checked_at} />.{" "}
          {scopeSentence(check.scope, check.recent_limit)}
        </span>
        <Button
          variant="secondary"
          icon="refresh"
          loading={running}
          hint="Reads every entry and compares the older ones with their digest. It can take a while on a large log."
          onClick={() => (full.data ? void full.refetch() : onCheckFull())}
        >
          {full.data ? "Check again" : "Check the whole log"}
        </Button>
      </div>
    </Stack>
  );
}
