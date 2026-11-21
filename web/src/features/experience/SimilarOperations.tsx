import { useSimilarOperations, type SimilarSource } from "../../api/hooks/experience";
import {
  QueryBoundary,
  Section,
  Skeleton,
  Stack,
  Text,
  Visible,
  useVisibility,
} from "../../components";
import { PastOperationCard } from "./PastOperationCard";
import styles from "./experience.module.css";

/** The sentence that opens the panel and every other view of past operations. */
const BODY =
  "Historical evidence, not authorization. A past fix is a hint about where to look; it never approves anything now.";

function Body({ source }: { source: SimilarSource }) {
  const query = useSimilarOperations(source);
  return (
    <Stack gap={3}>
      <Text muted size="sm">
        {BODY}
      </Text>
      <QueryBoundary
        query={query}
        context="load similar operations"
        loading={<Skeleton shape="rows" count={2} />}
        isEmpty={(data) => data.items.length === 0}
        empty={<Text muted>No similar past operations yet.</Text>}
      >
        {(data) => (
          <ul className={styles.list}>
            {data.items.map((item) => (
              <li key={item.operation.operation_id}>
                <PastOperationCard operation={item.operation} reasons={item.reasons} compact />
              </li>
            ))}
          </ul>
        )}
      </QueryBoundary>
    </Stack>
  );
}

/** With its own heading when the detail level shows it in place; the toggle's label otherwise. */
function Panel({ source }: { source: SimilarSource }) {
  const { shown } = useVisibility("P2");
  return shown ? (
    <Section title="Similar past operations">
      <Body source={source} />
    </Section>
  ) : (
    <Body source={source} />
  );
}

/**
 * Past operations that resemble an operation or an incident, as hints about where to look.
 * Technical detail: in Standard and Essential nothing is requested until it is opened.
 */
export function SimilarOperations({ source }: { source: SimilarSource }) {
  return (
    <Visible category="P2" id="experience.similar" label="Similar past operations">
      <Panel source={source} />
    </Visible>
  );
}
