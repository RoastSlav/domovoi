import { InlineMessage, StatusPill, Text } from "../../components";
import { coverageMeta } from "../../design/tone";
import { containmentBlock } from "./blocked";
import styles from "./paths.module.css";

/**
 * What is blocked on a host, with the reasons. It only displays a conclusion drawn from the
 * hub's data: nothing on this page decides or enforces anything.
 */
export function PathsBanner({
  host,
  coverageAvailable,
  states,
}: {
  coverageAvailable: boolean;
  host: string;
  states: readonly string[];
}) {
  const summary = containmentBlock({ coverageAvailable, states });
  if (summary.blocked) {
    return (
      <InlineMessage tone="warning" title={`Unattended containment is blocked on ${host}`}>
        <ul className={styles.reasons}>
          {summary.reasons.map((reason) => (
            <li key={reason}>{reason}</li>
          ))}
        </ul>
        <p>Monitoring and actions you approve yourself are not affected.</p>
      </InlineMessage>
    );
  }
  return (
    <InlineMessage tone="success" title={`Management paths are confirmed and current on ${host}`}>
      <p>
        Unattended containment also needs the capability, the policy and the helper to allow it.
        Monitoring and actions you approve yourself are not affected.
      </p>
    </InlineMessage>
  );
}

/** The state of the helper check and, in a sentence, what it means for confirming a path. */
export function CoverageRow({ available }: { available: boolean }) {
  return (
    <div className={styles.coverage}>
      <StatusPill meta={coverageMeta(available)} />
      <span className={styles.coverageText}>
        <Text muted>
          {available
            ? "When you confirm a path, the hub asks the helper whether it covers it."
            : "The check that shows the helper protects a path is not available in this version, so no path can be confirmed yet. You can add paths, accept the ones Domovoi found and retract wrong ones."}
        </Text>
      </span>
    </div>
  );
}
