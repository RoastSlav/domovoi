import { useMemo } from "react";
import type { ManagementPath } from "../../api/types";
import { useSession } from "../../api/hooks/session";
import { DataTable, RelativeTime, StatusPill, Tag, Text, type Column } from "../../components";
import { actorLabel, sourceInfo } from "../../design/knowledgeText";
import { pathStateMeta } from "../../design/tone";
import { PathActions } from "./PathActions";
import { valueText } from "./valueText";
import styles from "./paths.module.css";

function freshness(path: ManagementPath) {
  const until = path.fact.fresh_until;
  if (!until) {
    return path.state === "stale" ? "Stale" : "No freshness limit";
  }
  return path.state === "stale" ? (
    <>
      Stale since <RelativeTime at={until} />
    </>
  ) : (
    <>
      Fresh until <RelativeTime at={until} />
    </>
  );
}

/** A host's management paths as a table: state, source, freshness, confirmation and the owner's buttons. */
export function PathRows({
  paths,
  host,
  coverageAvailable,
}: {
  coverageAvailable: boolean;
  host: string;
  paths: readonly ManagementPath[];
}) {
  const me = useSession().data?.user.id;
  const columns = useMemo<Column<ManagementPath>[]>(
    () => [
      {
        key: "path",
        label: "Path",
        cell: (path) => (
          <span className={styles.cidr}>
            {valueText("management_path", path.fact.value, {
              hosts: new Map(),
              entities: new Map(),
            })}
          </span>
        ),
      },
      {
        key: "state",
        label: "State",
        cell: (path) => <StatusPill size="sm" meta={pathStateMeta(path.state)} />,
      },
      {
        key: "source",
        label: "Source",
        cell: (path) => (
          <Tag hint={sourceInfo(path.fact.source).hint}>{sourceInfo(path.fact.source).label}</Tag>
        ),
      },
      {
        key: "freshness",
        label: "Freshness",
        cell: (path) => <Text muted>{freshness(path)}</Text>,
      },
      {
        key: "confirmation",
        label: "Confirmation",
        cell: (path) =>
          path.confirmation ? (
            <Text muted>
              Confirmed by {actorLabel(path.confirmation.confirmed_by, me)}{" "}
              <RelativeTime at={path.confirmation.confirmed_at} />
            </Text>
          ) : (
            <Text muted>Not confirmed</Text>
          ),
      },
      {
        key: "actions",
        label: "Actions",
        hideLabel: true,
        cell: (path) => <PathActions path={path} coverageAvailable={coverageAvailable} />,
      },
    ],
    [me, coverageAvailable],
  );
  return (
    <DataTable
      caption={`Management paths of ${host}`}
      columns={columns}
      rows={paths}
      rowKey={(path) => path.fact.id}
      rowHref={(path) => `/knowledge/facts/${encodeURIComponent(path.fact.id)}`}
    />
  );
}
