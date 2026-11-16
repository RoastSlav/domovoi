import { Cluster, InfoTip, Tag } from "../../components";

/**
 * What the v2 plan code does and does not cover, in the exact words the approval surface
 * promises. Keep it honest: v2 binds the capability and version, the manifest and entry
 * script bytes, the host and its enrolled agent key, the incident subject, the arguments and
 * the execution id, not transitive dependencies or interpreters.
 */
export const PLAN_HASH_HINT_V2 =
  "This code identifies the plan you approve: the capability and its version, its manifest and entry script, the host and its enrolled agent key, the incident's check and target, the arguments and the execution id. If any of them changed, the approval is refused. It does not cover transitive dependencies, interpreters or other files, and it cannot prove the hub itself is honest.";

/** What a legacy v1 plan code covers: the older binding, kept for the action's whole life. */
export const PLAN_HASH_HINT_V1 =
  "This legacy code identifies what you approve: the skill, its version, the host and the arguments shown here. If any of them changed, the approval is refused. It does not cover the skill's script contents, and an action created before v2 keeps this binding.";

/** The label a legacy action carries, so no client has to infer the binding family. */
export const PLAN_BINDING_V1_LABEL = "v1 binding";

/** The short plan hash of a waiting action, with the explanation of what it binds. */
export function PlanTag({ planHash, binding }: { planHash: string; binding: string }) {
  const v2 = binding === "v2";
  return (
    <Cluster>
      <Tag>Plan {planHash.slice(0, 8)}</Tag>
      <Tag>{v2 ? "v2" : PLAN_BINDING_V1_LABEL}</Tag>
      <InfoTip
        title={v2 ? "Plan hash v2" : "Plan hash (v1 binding)"}
        learnMore="concepts/actions.html"
      >
        {v2 ? PLAN_HASH_HINT_V2 : PLAN_HASH_HINT_V1}
      </InfoTip>
    </Cluster>
  );
}
