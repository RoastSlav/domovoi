import { useState } from "react";
import { useHostLogs } from "../../api/hooks/hosts";
import { LOGS_REFRESH_MS } from "../../api/hooks/system";
import { LogViewer, DEFAULT_LEVEL, DEFAULT_LINES } from "../logs/LogViewer";

/** Recent log lines of host `host`'s agent, drawn like the Logs page reads them. */
export function LogsTab({ host }: { host: string }) {
  const [live, setLive] = useState(true);
  const logs = useHostLogs(host, DEFAULT_LEVEL, DEFAULT_LINES, true, live);
  return (
    <LogViewer
      query={logs}
      level={DEFAULT_LEVEL}
      label={`Log lines on ${host}`}
      refreshMs={live ? LOGS_REFRESH_MS : null}
      onLiveChange={setLive}
    />
  );
}
