import { useState } from "react";
import { Button } from "./Button";
import { Tag } from "./Tag";
import styles from "./ChatThread.module.css";

/** A read-only tool the model called while answering. */
export interface ChatTool {
  /** The host it looked at, when it was one host. */
  host?: string;
  name: string;
}

/** The tools behind one answer, folded behind a small "N tools used" toggle. */
export function ToolTags({ tools }: { tools: ChatTool[] }) {
  const [open, setOpen] = useState(false);
  return (
    <div className={styles.tools}>
      <Button size="sm" variant="ghost" aria-expanded={open} onClick={() => setOpen(!open)}>
        {tools.length === 1 ? "1 tool used" : `${tools.length} tools used`}
      </Button>
      {open &&
        tools.map((tool, index) => (
          <Tag key={`${tool.name}-${index}`}>
            {tool.host ? `${tool.name} · ${tool.host}` : tool.name}
          </Tag>
        ))}
    </div>
  );
}
