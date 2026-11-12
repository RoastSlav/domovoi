import type { Action } from "../../api/types";
import { Text } from "../../components";
import styles from "./approvals.module.css";

/**
 * What an action would run with, as `key = value` lines in mono, so a person sees the exact
 * arguments before approving. A skill without arguments says so instead of showing nothing.
 */
export function ActionArgs({ args }: { args: Action["args"] }) {
  const entries = Object.entries(args);
  if (entries.length === 0) {
    return (
      <Text muted size="sm" block>
        No arguments
      </Text>
    );
  }
  return (
    <div className={styles.args}>
      <p className={styles.argsLabel}>Arguments</p>
      <ul className={styles.argList} aria-label="Arguments">
        {entries.map(([name, value]) => (
          <li key={name}>
            {name} = {value}
          </li>
        ))}
      </ul>
    </div>
  );
}
