import { Cluster, InfoTip, Tag } from "../../components";

/**
 * What the plan code does and does not cover, in the exact words the approval surface
 * promises. Keep it honest: the v1 hash binds the skill, its manifest version, the host and
 * the arguments, not the skill's script contents or a resolved target.
 */
export const PLAN_HASH_HINT =
  "This code identifies what you approve: the skill, its version, the host and the arguments shown here. If any of them changed, the approval is refused. It does not cover the skill's script contents.";

/** The short plan hash of a waiting action, with the explanation of what it binds. */
export function PlanTag({ planHash }: { planHash: string }) {
  return (
    <Cluster>
      <Tag>Plan {planHash.slice(0, 8)}</Tag>
      <InfoTip title="Plan hash">{PLAN_HASH_HINT}</InfoTip>
    </Cluster>
  );
}
