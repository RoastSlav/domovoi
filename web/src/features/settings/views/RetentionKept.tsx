import { Link } from "react-router";
import { useSettings } from "../../../api/hooks/system";
import { InlineMessage, QueryBoundary, Skeleton, Text, Visible } from "../../../components";
import styles from "../settings.module.css";

/** What is kept with no period, what deletion does, and why a change asks for the password. */
export function RetentionKept() {
  const settings = useSettings();
  return (
    <QueryBoundary
      query={settings}
      context="load retention settings"
      loading={<Skeleton shape="rows" count={3} />}
    >
      {(s) => (
        <div className={styles.retentionKept}>
          <Visible category="P0" id="retention.kept">
            <Text strong block>
              Kept, with no period
            </Text>
            <ul>
              <li>Operations and their summaries</li>
              <li>
                The audit log, checked on the <Link to="/audit">Audit log</Link> page
              </li>
              <li>Incidents, digest facts and notes</li>
            </ul>
          </Visible>
          <Visible category="P0" id="retention.guarantee">
            <InlineMessage tone="info" title="What deletion does">
              When a period ends the data is removed from the live database, its search index and
              what the model sees. It stays in backups until they rotate out (
              {s.retention?.backups_keep ?? "several"} nightly and{" "}
              {s.retention?.backups_keep_pre_migrate ?? "a few"} before upgrades, set under
              Backups), and restoring a backup brings it back until the next daily clean-up.
            </InlineMessage>
          </Visible>
          <Visible category="P1" id="retention.stepup">
            <Text muted block>
              Changing a period asks for your password again, because a shorter period deletes data
              sooner.
            </Text>
          </Visible>
        </div>
      )}
    </QueryBoundary>
  );
}
