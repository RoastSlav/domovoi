import { memo } from "react";
import { Link } from "react-router";
import type { IncidentSummary } from "../../api/types";
import { Card, Cluster, RelativeTime, Stack, StatusPill, Text } from "../../components";
import { incidentStateMeta, severityMeta } from "../../design/tone";
import { triageSaid } from "./badge";
import { checkText } from "./incidentColumns";
import { IncidentRowMenu } from "./IncidentRowMenu";
import styles from "./incidents.module.css";

/** One incident as a card, for narrow screens: the table's facts in the table's order. */
export const IncidentCardRow = memo(function IncidentCardRow({
  incident,
  host,
  hideStatus,
}: {
  /** The group already says the status, as in Action needed. */
  hideStatus: boolean;
  incident: IncidentSummary;
  host: string | undefined;
}) {
  const state = incidentStateMeta(incident);
  const note = triageSaid(incident);
  return (
    <li>
      <Card tone={severityMeta(incident.severity).tone}>
        <Stack gap={2}>
          <Cluster justify="between" align="start" wrap={false}>
            <Stack gap={1}>
              {!hideStatus && (
                <Cluster>
                  <StatusPill meta={state} />
                </Cluster>
              )}
              <Link to={`/incidents/${incident.id}`} className={styles.titleLink}>
                <Text strong>{incident.title}</Text>
              </Link>
            </Stack>
            <IncidentRowMenu incident={incident} />
          </Cluster>
          <Cluster>
            <Text size="sm">{host ?? "Unknown host"}</Text>
            <Text mono size="sm">
              {checkText(incident)}
            </Text>
            <StatusPill size="sm" meta={severityMeta(incident.severity)} />
          </Cluster>
          <Cluster>
            <Text muted size="sm">
              Last fired <RelativeTime at={incident.last_firing_at} />
            </Text>
            {incident.occurrences > 1 && (
              <Text muted size="sm">
                {incident.occurrences} times
              </Text>
            )}
            {note && (
              <Text muted size="sm">
                {note}
              </Text>
            )}
          </Cluster>
        </Stack>
      </Card>
    </li>
  );
});
