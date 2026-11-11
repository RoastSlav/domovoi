import type { ReactNode } from "react";
import { InlineMessage } from "../components";
import { useSelectedHost } from "./HostContext";
import styles from "./Layout.module.css";

/**
 * On a page that does not follow the host selector, says so while a host is selected, so
 * nobody takes the whole fleet for one host. `children` says why in a few words. Renders
 * nothing under "All hosts".
 */
export function AllHostsNote({ children }: { children: ReactNode }) {
  const [selected] = useSelectedHost();
  if (selected === undefined) {
    return null;
  }
  return (
    <div className={styles.allHostsNote}>
      <InlineMessage tone="info" title="Showing all hosts">
        {children}
      </InlineMessage>
    </div>
  );
}
