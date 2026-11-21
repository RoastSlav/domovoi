import type { KnowledgeFact } from "../../api/types";
import { KeyValueList, RelativeTime } from "../../components";

function at(time: string | null | undefined, none: string) {
  return time ? <RelativeTime at={time} /> : none;
}

/** When a fact was observed, checked, recorded and when it stops being fresh or valid. */
export function FactTimes({ fact }: { fact: KnowledgeFact }) {
  return (
    <KeyValueList
      items={[
        { label: "Observed", value: at(fact.observed_at, "Never") },
        { label: "Last verified", value: at(fact.last_verified_at, "Never") },
        { label: "Fresh until", value: at(fact.fresh_until, "No limit") },
        { label: "Valid until", value: at(fact.valid_until, "No limit") },
        { label: "Recorded", value: at(fact.created_at, "Never") },
      ]}
    />
  );
}
