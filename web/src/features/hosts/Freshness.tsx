import type { Fact } from "../../api/types";
import { RelativeTime, Text } from "../../components";
import { newest, STALE_MS } from "./factValue";
import styles from "./overview.module.css";

/** Marks a fact older than an hour with its age, in the warning tone; a fresh one has no mark. */
export function Stale({ at }: { at: string }) {
  const age = new Date().getTime() - new Date(at).getTime();
  return age > STALE_MS ? (
    <span className={styles.stale}>
      Stale: <RelativeTime at={at} />
    </span>
  ) : null;
}

/** One muted line saying when the newest of `facts` was observed; nothing for no facts. */
export function Updated({ facts }: { facts: readonly Pick<Fact, "observed_at">[] }) {
  const at = newest(facts as Fact[]);
  return at ? (
    <Text muted size="sm">
      Updated <RelativeTime at={at} />
    </Text>
  ) : null;
}
