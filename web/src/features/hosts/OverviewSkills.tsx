import { Link } from "react-router";
import { useSkills } from "../../api/hooks/hosts";
import {
  Cluster,
  EmptyState,
  QueryBoundary,
  Skeleton,
  Stack,
  StatusPill,
  Tag,
  Text,
} from "../../components";
import { riskMeta } from "../../design/tone";
import styles from "./hosts.module.css";

/** How many skills the overview lists; the rest are one link away. */
const LISTED = 6;

/** The skills the host's agent reported, one line each with its risk; managing them is one link away. */
export function OverviewSkills({ host }: { host: string }) {
  const skills = useSkills(host);
  return (
    <QueryBoundary
      query={skills}
      context="load skills"
      loading={<Skeleton shape="rows" count={3} />}
      isEmpty={(list) => list.length === 0}
      empty={
        <EmptyState icon="tool" title="No skills">
          <p>An agent sends its skills when it connects, so check that {host} is connected.</p>
        </EmptyState>
      }
    >
      {(list) => (
        <Stack gap={3}>
          <ul className={styles.skills} aria-label={`Skills on ${host}`}>
            {list.slice(0, LISTED).map((skill) => (
              <li key={skill.name}>
                <Cluster gap={2}>
                  <Text mono strong>
                    {skill.name}
                  </Text>
                  <StatusPill size="sm" meta={riskMeta(skill.risk)} />
                  {!skill.enabled && <Tag>Off</Tag>}
                </Cluster>
                <Text muted size="sm" block>
                  {skill.description}
                </Text>
              </li>
            ))}
          </ul>
          <Link to={`/skills?host=${encodeURIComponent(host)}`}>
            Manage skills on {host}
            {list.length > LISTED && ` (${list.length} in all)`}
          </Link>
        </Stack>
      )}
    </QueryBoundary>
  );
}
