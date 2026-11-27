import type { RetiredFact } from "../../api/types";
import { KeyValueList, RelativeTime, Section } from "../../components";
import { withUnit } from "./factValue";

/**
 * Facts a check used to report and no longer does, for example a disk that was unmounted. They
 * are kept (the history stays searchable) but never shown as current values.
 */
export function NoLongerReported({ facts }: { facts: readonly RetiredFact[] }) {
  if (facts.length === 0) {
    return null;
  }
  return (
    <Section
      title={`No longer reported (${facts.length})`}
      collapsed
      description="The host's checks stopped reporting these. The last value is shown, not a current one."
    >
      <KeyValueList
        items={facts.map((one) => ({
          label: one.fact.label,
          value: (
            <>
              last {withUnit(one.fact)}, stopped <RelativeTime at={one.retired_at} />
            </>
          ),
        }))}
      />
    </Section>
  );
}
