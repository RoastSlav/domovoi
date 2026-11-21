import { Text, Tooltip } from "../../components";
import { planCode } from "./jobModel";

/**
 * The plan code the owner reviews: the first eight characters of the plan hash. The
 * approval given in the dialog is bound to the full hash behind it.
 *
 * Not for editing or comparing hashes: the page never computes one.
 */
export function PlanCode({ hash }: { hash: string }) {
  return (
    <Tooltip content="Your approval is bound to this plan. If anything changes, you review it again.">
      <span>
        <Text mono strong>{`Plan ${planCode(hash)}`}</Text>
      </span>
    </Tooltip>
  );
}
