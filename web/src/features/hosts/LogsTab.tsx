import { useHostLogs } from "../../api/hooks/hosts";
import { LogViewer, DEFAULT_LEVEL, DEFAULT_LINES } from "../logs/LogViewer";

/** Recent log lines of host `host`'s agent, drawn like the Logs page reads them. */
export function LogsTab({ host }: { host: string }) {
  const logs = useHostLogs(host, DEFAULT_LEVEL, DEFAULT_LINES);
  return (
    <LogViewer
      logs={{
        items: logs.data ?? [],
        error: logs.error,
        loading: logs.isPending,
        refresh: () => logs.refetch(),
      }}
      level={DEFAULT_LEVEL}
      label={`Log lines on ${host}`}
    />
  );
}
