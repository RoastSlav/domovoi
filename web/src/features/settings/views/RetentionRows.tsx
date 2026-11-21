import type { RetentionSettings } from "../../../api/types";
import { Tag, Text } from "../../../components";
import { RETENTION_LABELS } from "../sections/retention";
import { SettingsCard, ValueRow } from "../SettingRow";

const days = (value: number): string => (value === 1 ? "1 day" : `${value} days`);

/** The retention values of a hub managed by file, read only. */
export function RetentionRows({ retention }: { retention: RetentionSettings | undefined }) {
  const entries = retention?.entries ?? [];
  if (!retention || entries.length === 0) {
    return <Text muted>This hub did not report its retention settings.</Text>;
  }
  return (
    <SettingsCard>
      {entries.map((entry) => (
        <ValueRow key={entry.key} label={`${RETENTION_LABELS[entry.key] ?? entry.key} (days)`}>
          {days(entry.days)}
          {entry.applies === "later" && (
            <>
              {" "}
              <Tag>Not applied yet</Tag>{" "}
              {entry.applies_once && <Text muted>Applies once {entry.applies_once}.</Text>}
            </>
          )}
        </ValueRow>
      ))}
      <ValueRow label="Nightly backups kept">{retention.backups_keep}</ValueRow>
      <ValueRow label="Pre-upgrade backups kept">{retention.backups_keep_pre_migrate}</ValueRow>
    </SettingsCard>
  );
}
