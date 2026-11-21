import type { KnowledgeRelation } from "../../api/types";
import { Button, Dialog, KeyValueList, RelativeTime, StatusPill, Text } from "../../components";
import { actorLabel, relationPhrase, sourceInfo } from "../../design/knowledgeText";
import { knowledgeStatusMeta } from "../../design/tone";
import { useSession } from "../../api/hooks/session";
import { EvidenceRef } from "./EvidenceRef";
import { SubjectLink } from "./SubjectLink";
import type { SubjectNames } from "./subjects";
import { REMOVED_VALUE } from "./FactsTable";

const LIVE = new Set(["current", "stale", "disputed"]);

/** Whether a relation can still be retracted: its effective status is live. */
export function relationIsLive(relation: KnowledgeRelation): boolean {
  return LIVE.has(relation.status);
}

/** One relation in full, read-only, with Retract when it is live. */
export function RelationDialog({
  relation,
  names,
  onClose,
  onRetract,
}: {
  names: SubjectNames;
  onClose: () => void;
  onRetract: (relation: KnowledgeRelation) => void;
  relation: KnowledgeRelation;
}) {
  const session = useSession();
  const me = session.data?.user.id;
  const end = (subject: string | null | undefined) =>
    subject ? <SubjectLink subject={subject} names={names} /> : <Text muted>{REMOVED_VALUE}</Text>;
  const items = [
    { label: "From", value: end(relation.from) },
    { label: "Relation", value: relationPhrase(relation.kind) },
    { label: "To", value: end(relation.to) },
    { label: "Source", value: sourceInfo(relation.source).label },
    { label: "Status", value: <StatusPill meta={knowledgeStatusMeta(relation.status)} /> },
    { label: "Observed", value: <RelativeTime at={relation.observed_at} /> },
    {
      label: "Evidence",
      value: relation.evidence ? <EvidenceRef evidence={relation.evidence} /> : "None",
    },
    { label: "Recorded by", value: actorLabel(relation.actor, me) },
    { label: "Recorded", value: <RelativeTime at={relation.created_at} /> },
    ...(relation.retracted_at
      ? [{ label: "Retracted", value: <RelativeTime at={relation.retracted_at} /> }]
      : []),
  ];
  return (
    <Dialog
      title="Relation"
      onClose={onClose}
      actions={
        <>
          {relationIsLive(relation) && (
            <Button variant="secondary" onClick={() => onRetract(relation)}>
              Retract
            </Button>
          )}
          <Button onClick={onClose}>Close</Button>
        </>
      }
    >
      <KeyValueList items={items} />
    </Dialog>
  );
}
