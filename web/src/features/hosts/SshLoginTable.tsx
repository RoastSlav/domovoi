import {
  DataTable,
  EmptyState,
  RelativeTime,
  StatusPill,
  Tag,
  Text,
  type Column,
} from "../../components";
import { sshLoginMeta } from "../../design/tone";
import { resultOf, type LoginEntry } from "./sshLogins";

function columns(all: readonly LoginEntry[]): Column<LoginEntry>[] {
  return [
    {
      key: "user",
      label: "User",
      cell: (entry) => <Text strong>{entry.user}</Text>,
    },
    {
      key: "address",
      label: "Source address",
      cell: (entry) =>
        entry.address === "" ? (
          <Text muted>unknown</Text>
        ) : (
          <>
            <Text mono>{entry.address}</Text>
            {entry.network !== "" && (
              <Tag {...(entry.network === "wan" ? { tone: "warning" as const } : {})}>
                {entry.network === "wan" ? "Internet" : "Local network"}
              </Tag>
            )}
          </>
        ),
    },
    {
      key: "time",
      label: "Last login",
      cell: (entry) => <RelativeTime at={entry.at} />,
    },
    {
      key: "count",
      label: "Logins",
      numeric: true,
      cell: (entry) => entry.count,
    },
    {
      key: "result",
      label: "Result",
      cell: (entry) => <StatusPill size="sm" meta={sshLoginMeta(resultOf(entry, all))} />,
    },
  ];
}

/**
 * The login history: who logged in, from which address, when, and how it ended. `all` is
 * the whole history, so a failed login can tell whether its address keeps failing; `shown`
 * is what the filters left.
 */
export function SshLoginTable({
  all,
  host,
  shown,
}: {
  all: readonly LoginEntry[];
  host: string;
  shown: readonly LoginEntry[];
}) {
  return (
    <DataTable
      caption={`SSH logins on ${host}`}
      columns={columns(all)}
      rows={shown}
      rowKey={(entry) => entry.id}
      rowTone={(entry) => {
        const tone = sshLoginMeta(resultOf(entry, all)).tone;
        return tone === "danger" ? tone : undefined;
      }}
      empty={
        <EmptyState icon="filter" title="No logins match these filters">
          <p>Choose All results and Anywhere to see every login again.</p>
        </EmptyState>
      }
    />
  );
}
