import { useState } from "react";
import { useConversations } from "../../api/hooks/conversations";
import { Button, OptionPicker, Stack } from "../../components";
import { NewConversation } from "./NewConversation";
import { OperatorConversation } from "./OperatorConversation";
import { readActive, writeActive } from "./active";
import styles from "./operator.module.css";

/** How many conversations the panel's picker offers. */
const PICKER_SIZE = 10;

/**
 * The Operator in the side panel: a picker over the newest conversations, a way to start a
 * new one, and the open conversation. It keeps working on every page; the draft and the
 * chosen conversation survive closing it, and closing it never stops an answer.
 */
export function OperatorPanel() {
  const [active, setActive] = useState<string | undefined>(readActive);
  const list = useConversations();
  const choose = (id: string | undefined) => {
    writeActive(id);
    setActive(id);
  };
  const options = (list.data?.items ?? []).slice(0, PICKER_SIZE).map((item) => ({
    value: item.id,
    label: item.title || "New conversation",
  }));
  return (
    <Stack gap={3}>
      <div className={styles.panelHeader}>
        {active !== undefined && options.length > 0 && (
          <OptionPicker
            label="Conversation"
            value={active}
            options={options}
            onChange={(id) => choose(id)}
          />
        )}
        <Button size="sm" variant="secondary" onClick={() => choose(undefined)}>
          New conversation
        </Button>
      </div>
      {active === undefined ? (
        <NewConversation onCreated={choose} />
      ) : (
        <OperatorConversation key={active} id={active} />
      )}
    </Stack>
  );
}
