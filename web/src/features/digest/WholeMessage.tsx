import { useState } from "react";
import { Button, Card, Markdown, Stack } from "../../components";
import { Icon } from "../../design/icons";
import styles from "./digest.module.css";

/**
 * The digest message exactly as Discord gets it. When the host cards above already say the
 * same thing it starts folded, so the page is not read twice.
 */
export function WholeMessage({
  markdown,
  collapsible,
}: {
  markdown: string;
  collapsible: boolean;
}) {
  const [open, setOpen] = useState(false);
  const shown = !collapsible || open;
  return (
    <Card>
      <Stack gap={2}>
        {collapsible && (
          <h3 className={styles.cardTitle}>
            <Icon name="digest" size={16} />
            The whole message, as it is sent
          </h3>
        )}
        {collapsible && (
          <Button size="sm" variant="ghost" aria-expanded={open} onClick={() => setOpen(!open)}>
            {open ? "Hide the whole message" : "Show the whole message as sent"}
          </Button>
        )}
        {shown && <Markdown text={markdown} breaks />}
      </Stack>
    </Card>
  );
}
