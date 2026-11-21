import type { PolicyVersionSummary } from "../../api/types";
import { Cluster, KeyValueList, RelativeTime, Text } from "../../components";
import { actorText } from "./ruleText";

/** What a policy without a published version means. */
export const NO_POLICY_TEXT =
  "No policy is published yet. Without rules, reads can run automatically and everything else asks you. Start a draft to write the first rules.";

/**
 * The facts of a published version: number, who published it and when, and the rule counts.
 * @param version The version summary, or nothing when none is published.
 * @param zone The hub's time-window zone.
 */
export function ActiveSummary({
  version,
  zone,
}: {
  version: PolicyVersionSummary | null | undefined;
  zone: string;
}) {
  if (!version) return <Text>{NO_POLICY_TEXT}</Text>;
  return (
    <Cluster gap={4}>
      <KeyValueList
        items={[
          { label: "Active version", value: `Version ${version.number}` },
          {
            label: "Published",
            value: (
              <>
                by {actorText(version.published_by)} <RelativeTime at={version.published_at} />
              </>
            ),
          },
          {
            label: "Rules",
            value: `${version.rule_count} rules, ${version.locked_count} locked, ${version.shadow_count} shadow`,
          },
          { label: "Time zone", value: zone },
        ]}
      />
    </Cluster>
  );
}
