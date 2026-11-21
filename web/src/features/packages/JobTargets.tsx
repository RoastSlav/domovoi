import type { InstallJob } from "../../api/types";
import { currentTargets, earlierAttempts } from "./jobModel";
import { TargetRow } from "./TargetRow";
import styles from "./wizard.module.css";

/** The hosts of a job, each at its latest attempt, in host name order. */
export function JobTargets({ job }: { job: InstallJob }) {
  return (
    <ul className={styles.targetList} aria-label="Hosts">
      {currentTargets(job).map((target) => (
        <TargetRow
          key={target.host_id}
          job={job}
          target={target}
          earlier={earlierAttempts(job, target.host_id)}
        />
      ))}
    </ul>
  );
}
