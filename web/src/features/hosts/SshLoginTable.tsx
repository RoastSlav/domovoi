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
import { displayAddress } from "./ipAddress";
import { resultOf, type LoginEntry } from "./sshLogins";

function columns(all: readonly LoginEntry[]): Column<LoginEntry>[] {
  return [
    {
      key: "user",
      label: "User",
      width: "10rem",
      truncate: "ellipsis",
      tip: (entry) => entry.user,
      cell: (entry) => <Text strong>{entry.user}</Text>,
    },
    {
      key: "address",
      label: "Source address",
      width: "16rem",
      truncate: "ellipsis",
      tip: (entry) => entry.address,
      cell: (entry) =>
        entry.address === "" ? (
          <Text muted>unknown</Text>
        ) : (
          <Text mono>{displayAddress(entry.address)}</Text>
        ),
    },
    {
      key: "network",
      label: "Network",
      width: "10rem",
      cell: (entry) =>
        entry.network === "" ? (
          <Text muted>unknown</Text>
        ) : (
          <Tag {...(entry.network === "wan" ? { tone: "warning" as const } : {})}>
            {entry.network === "wan" ? "Internet" : "Local network"}
          </Tag>
        ),
    },
    {
      key: "time",
      label: "Last login",
      width: "9rem",
      cell: (entry) => <RelativeTime at={entry.at} />,
    },
    {
      key: "count",
      label: "Logins",
      width: "6rem",
      numeric: true,
      cell: (entry) => entry.count,
    },
    {
      key: "result",
      label: "Result",
      width: "9rem",
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
      layout="fixed"
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
