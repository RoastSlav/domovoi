import { Cluster, InfoTip, Tag } from "../../components";

/**
 * What the plan code does and does not cover, in the exact words the approval surface
 * promises. Keep it honest: it binds the capability and version, the manifest and entry
 * script bytes, the host and its enrolled agent key, the incident subject, the arguments and
 * the execution id, not transitive dependencies or interpreters.
 */
export const PLAN_HASH_HINT =
  "This code identifies the plan you approve: the capability and its version, its manifest and entry script, the host and its enrolled agent key, the incident's check and target, the arguments and the execution id. If any of them changed, the approval is refused. It does not cover transitive dependencies, interpreters or other files, and it cannot prove the hub itself is honest.";

/** The short plan hash of a waiting step, with the explanation of what it binds. */
export function PlanTag({ planHash }: { planHash: string }) {
  return (
    <Cluster>
      <Tag>Plan {planHash.slice(0, 8)}</Tag>
      <InfoTip title="Plan code" learnMore="concepts/operations.html#approval">
        {PLAN_HASH_HINT}
      </InfoTip>
    </Cluster>
  );
}
