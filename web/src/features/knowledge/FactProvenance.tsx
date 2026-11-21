import { Link } from "react-router";
import { useSession } from "../../api/hooks/session";
import type { KnowledgeFact } from "../../api/types";
import { KeyValueList } from "../../components";
import { actorLabel, sourceInfo } from "../../design/knowledgeText";
import { EvidenceRef } from "./EvidenceRef";

/** Where a fact came from: its source, who recorded it, what it was derived from, what it replaced. */
export function FactProvenance({ fact }: { fact: KnowledgeFact }) {
  const session = useSession();
  const source = sourceInfo(fact.source);
  return (
    <KeyValueList
      items={[
        { label: "Source", value: source.label, hint: source.hint },
        { label: "Recorded by", value: actorLabel(fact.actor, session.data?.user.id) },
        {
          label: "Evidence",
          value: fact.evidence ? <EvidenceRef evidence={fact.evidence} /> : "None",
        },
        {
          label: "Earlier value",
          value: fact.supersedes ? (
            <Link to={`/knowledge/facts/${encodeURIComponent(fact.supersedes)}`}>
              The value this one replaced
            </Link>
          ) : (
            "None"
          ),
        },
      ]}
    />
  );
}
