import type { KnowledgeFact } from "../../api/types";
import { Cluster, InfoTip, PageHeader, StatusPill, Tag, Visible } from "../../components";
import { predicateInfo, sourceInfo } from "../../design/knowledgeText";
import { knowledgeStatusMeta } from "../../design/tone";
import { factActions } from "./factActions";
import { FactActions } from "./FactButtons";
import type { SubjectNames } from "./subjects";

/** The title, status, source and owner buttons at the top of a fact's page. */
export function FactHeader({
  fact,
  title,
  names,
}: {
  fact: KnowledgeFact;
  names: SubjectNames;
  title: string;
}) {
  const allowed = factActions(fact);
  const source = sourceInfo(fact.source);
  const safety = predicateInfo(fact.predicate).safety;
  return (
    <PageHeader
      title={title}
      docs="concepts/knowledge.html"
      topic="knowledge"
      meta={
        <Visible category="P0" id="fact.status">
          <Cluster gap={2}>
            <StatusPill meta={knowledgeStatusMeta(fact.status)} />
            <Tag>{source.label}</Tag>
            {source.hint && <InfoTip title={source.label}>{source.hint}</InfoTip>}
            {safety && <Tag>Safety-related</Tag>}
            {safety && (
              <InfoTip title="Safety-related">
                This kind of fact is used for a safety decision only when you entered it and it is
                current.
              </InfoTip>
            )}
          </Cluster>
        </Visible>
      }
      actions={
        allowed.accept || allowed.verify || allowed.retract ? (
          <Visible category="P0" id="fact.actions">
            <FactActions fact={fact} names={names} />
          </Visible>
        ) : undefined
      }
    />
  );
}
