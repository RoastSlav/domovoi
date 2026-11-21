import { Link, useParams } from "react-router";
import { useFact } from "../../api/hooks/knowledge";
import { ApiFailure, type KnowledgeFact } from "../../api/types";
import { usePageName } from "../../app/PageTitle";
import {
  Card,
  DetailsScope,
  EmptyState,
  LoadError,
  PageHeader,
  Skeleton,
  Stack,
  Text,
  Visible,
  VisibilityPage,
} from "../../components";
import { predicateInfo, SAFETY_CRITICAL } from "../../design/knowledgeText";
import { FactHeader } from "./FactHeader";
import { FactHistory } from "./FactHistory";
import { FactNotes } from "./FactNotes";
import { FactProvenance } from "./FactProvenance";
import { FactSiblings } from "./FactSiblings";
import { FactTimes } from "./FactTimes";
import { factValueText } from "./FactsTable";
import { subjectLabel, type SubjectNames } from "./subjects";
import { useSubjectLabels } from "./useSubjectLabels";

function Missing() {
  return (
    <EmptyState
      icon="info"
      title="No such fact"
      action={<Link to="/knowledge">Back to Knowledge</Link>}
    >
      This fact does not exist, or the link is wrong.
    </EmptyState>
  );
}

function Body({ fact, names, title }: { fact: KnowledgeFact; names: SubjectNames; title: string }) {
  return (
    <>
      <FactHeader fact={fact} names={names} title={title} />
      <Stack gap={4}>
        <FactNotes fact={fact} names={names} />
        <Visible category="P1" id="fact.value">
          <Card>
            <Text mono={SAFETY_CRITICAL.has(fact.predicate)} block>
              {factValueText(fact, names)}
            </Text>
          </Card>
        </Visible>
        <FactSiblings fact={fact} names={names} />
        <Visible category="P2" id="fact.origin" label="Where it came from">
          <FactProvenance fact={fact} />
        </Visible>
        <DetailsScope>
          <Visible category="P3" id="fact.times" label="Times" count={5}>
            <FactTimes fact={fact} />
          </Visible>
        </DetailsScope>
        <Visible category="P2" id="fact.history" label="History">
          <FactHistory fact={fact} names={names} />
        </Visible>
      </Stack>
    </>
  );
}

/**
 * One fact: its value, status and source, who recorded it and from what, the other values of
 * the same thing, when it was seen and what it replaced, and the owner's buttons.
 */
export function FactDetailPage() {
  const { id = "" } = useParams();
  const fact = useFact(id);
  const names = useSubjectLabels();
  const data = fact.data;
  const title = data
    ? `${predicateInfo(data.predicate).label}: ${subjectLabel(data.subject, names).text}`
    : undefined;
  usePageName(title);
  const notFound = fact.error instanceof ApiFailure && fact.error.status === 404;
  return (
    <VisibilityPage page="knowledge-detail">
      <Visible category="P0" id="fact.state">
        {notFound ? (
          <>
            <PageHeader title="Fact" />
            <Missing />
          </>
        ) : data && title ? (
          <Body fact={data} names={names} title={title} />
        ) : (
          <>
            <PageHeader title="Fact" />
            {fact.error ? (
              <LoadError error={fact.error} context="load the fact" onRetry={fact.refetch} />
            ) : (
              <Skeleton shape="rows" count={4} />
            )}
          </>
        )}
      </Visible>
    </VisibilityPage>
  );
}
