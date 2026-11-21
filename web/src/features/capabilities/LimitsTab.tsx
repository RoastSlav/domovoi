import { useSelectedHost } from "../../app/HostContext";
import { useHostCeiling } from "../../api/hooks/ceiling";
import type { HostCeiling } from "../../api/types";
import {
  DetailsScope,
  InlineMessage,
  QueryBoundary,
  Skeleton,
  Stack,
  StatusPill,
  Text,
  Tooltip,
  Visible,
} from "../../components";
import { ceilingReportingMeta } from "../../design/tone";
import { ChooseHost } from "../skills/ChooseHost";
import { LimitsTable } from "./LimitsTable";
import { CEILING_READ_ONLY, ceilingNotReported } from "./limitsText";

/** What the reporting state means for this host, in words. */
function Status({ answer }: { answer: HostCeiling }) {
  const host = answer.host;
  const meta = ceilingReportingMeta(answer.reporting);
  let body;
  if (answer.reporting === "reported") {
    body = answer.unattended_enabled
      ? "Unattended actions are allowed within the limits below."
      : "Unattended execution is off on this host. Nothing runs without your approval.";
  } else if (answer.reporting === "not_reported") {
    body = answer.connected
      ? ceilingNotReported(host)
      : `${host} is offline, so it has not reported its limits to this hub. They arrive when its agent connects.`;
  } else if (answer.reporting === "unreadable") {
    return (
      <InlineMessage tone="warning" title={`The hub could not read what ${host} reported.`}>
        {answer.unreadable_reason && <p>{answer.unreadable_reason}</p>}
        <p>Until it can, the hub treats unattended actions on this host as not allowed.</p>
      </InlineMessage>
    );
  } else {
    body = "This page does not know how to read that.";
  }
  return (
    <Stack gap={2}>
      <div>
        <StatusPill meta={meta} />
      </div>
      <Text block>{body}</Text>
    </Stack>
  );
}

/** The configuration fingerprint: the first 12 characters, the whole value in a tooltip. */
function Fingerprint({ hash }: { hash: string }) {
  return (
    <Text block size="sm">
      Configuration fingerprint{" "}
      <Tooltip content={hash}>
        <Text mono size="sm">
          <span tabIndex={0}>{hash.slice(0, 12)}</span>
        </Text>
      </Tooltip>
      . Lets Domovoi notice that the host&apos;s limits changed. It is not a check that they are
      right.
    </Text>
  );
}

function Limits({ answer }: { answer: HostCeiling }) {
  const reported = answer.reporting === "reported";
  return (
    <Stack gap={4}>
      <Visible category="P0" id="limits.note">
        <InlineMessage tone="info" title="These limits belong to the host">
          {CEILING_READ_ONLY}
        </InlineMessage>
      </Visible>
      <Visible category="P0" id="limits.status">
        <Status answer={answer} />
      </Visible>
      {reported && answer.entries.length > 0 && (
        <Visible category="P1" id="limits.table">
          <LimitsTable host={answer.host} entries={answer.entries} />
        </Visible>
      )}
      {reported && answer.unattended_enabled && answer.entries.length === 0 && (
        <Text
          block
        >{`${answer.host} reports no capability that may run unattended, so none does.`}</Text>
      )}
      {reported && answer.hash && (
        <DetailsScope>
          <Visible category="P3" id="limits.hash" label="Configuration fingerprint">
            <Fingerprint hash={answer.hash} />
          </Visible>
        </DetailsScope>
      )}
    </Stack>
  );
}

function ForHost({ host }: { host: string }) {
  const ceiling = useHostCeiling(host);
  return (
    <QueryBoundary
      query={ceiling}
      context="load the host limits"
      loading={<Skeleton shape="rows" />}
    >
      {(answer) => <Limits answer={answer} />}
    </QueryBoundary>
  );
}

/**
 * What the selected host allows to run unattended, exactly as it reported. Read only: the
 * host owns these limits, and nothing here can change them.
 */
export function LimitsTab() {
  const [selected] = useSelectedHost();
  if (selected === undefined) {
    return (
      <ChooseHost prompt="Choose a host to see its unattended limits." query={{ tab: "limits" }} />
    );
  }
  return <ForHost host={selected} />;
}
