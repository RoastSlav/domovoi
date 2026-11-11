import type { Settings } from "../../../api/types";
import { InlineMessage, Tag } from "../../../components";

/** Whether the hub's outgoing heartbeat is getting through. */
export function HeartbeatStatus({ heartbeat }: { heartbeat: Settings["heartbeat"] }) {
  if (heartbeat.failing) {
    return (
      <InlineMessage tone="danger" title="The heartbeat is failing">
        <p>
          The hub could not reach the heartbeat URL. Check the address in the Heartbeat section.
        </p>
      </InlineMessage>
    );
  }
  return heartbeat.configured ? <Tag tone="success">Working</Tag> : <Tag>Not configured</Tag>;
}
