import type { Settings } from "../../../api/types";
import { InlineMessage, Text } from "../../../components";
import { Icon } from "../../../design/icons";
import styles from "../settings.module.css";

/** Whether the hub's outgoing heartbeat is getting through: a message only when it is not. */
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
  if (!heartbeat.configured) {
    return (
      <Text muted block>
        The heartbeat is not configured.
      </Text>
    );
  }
  return (
    <p className={styles.okLine}>
      <Icon name="check-circle" size={16} />
      The heartbeat is working.
    </p>
  );
}
