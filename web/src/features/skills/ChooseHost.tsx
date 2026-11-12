import { useHosts } from "../../api/hooks/hosts";
import type { Host } from "../../api/types";
import { Card, EmptyState, Grid, QueryBoundary, Stack, StatusPill, Text } from "../../components";
import { hostConnectionMeta, hostConnectionOf } from "../../design/tone";
import { Icon } from "../../design/icons";
import styles from "./skills.module.css";

/** Hosts with an agent first; a hub-only host has no skills, so it goes last. */
function skillHostOrder(hosts: readonly Host[]): Host[] {
  const rank = (host: Host) => (hostConnectionOf(host) === "hub_only" ? 1 : 0);
  return [...hosts].sort(
    (a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name, "en", { numeric: true }),
  );
}

/** The name and connection of a host card, the part both kinds of card share. */
function Head({ host }: { host: Host }) {
  return (
    <div className={styles.hostCardHead}>
      <Text strong>{host.name}</Text>
      <StatusPill size="sm" meta={hostConnectionMeta(hostConnectionOf(host))} />
    </div>
  );
}

/**
 * Under "All hosts": a card per host that selects it and shows its skills. A host with no
 * agent has no skills to see, so its card is not a link and says why in words.
 */
export function ChooseHost() {
  const hosts = useHosts();
  return (
    <QueryBoundary
      query={hosts}
      context="load hosts"
      isEmpty={(list) => list.length === 0}
      empty={
        <EmptyState icon="server" title="No hosts yet">
          <p>Skills come from an agent. Add a host on the Hosts page first.</p>
        </EmptyState>
      }
    >
      {(list) => (
        <Stack gap={4}>
          <Text muted block>
            Choose a host to see its skills.
          </Text>
          <nav aria-label="Hosts">
            <Grid min="sm" gap={3}>
              {skillHostOrder(list).map((host) =>
                hostConnectionOf(host) === "hub_only" ? (
                  <Card key={host.id}>
                    <div className={styles.hostCard}>
                      <Head host={host} />
                      <Text muted size="sm" block>
                        No agent here, so no skills.
                      </Text>
                    </div>
                  </Card>
                ) : (
                  <Card key={host.id} to={`/skills?host=${encodeURIComponent(host.name)}`}>
                    <div className={styles.hostCard}>
                      <Head host={host} />
                      <span className={styles.hostGo}>
                        See its skills
                        <Icon name="chevron-right" size={14} />
                      </span>
                    </div>
                  </Card>
                ),
              )}
            </Grid>
          </nav>
        </Stack>
      )}
    </QueryBoundary>
  );
}
