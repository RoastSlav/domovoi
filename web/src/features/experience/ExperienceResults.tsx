import { useExperienceSearch } from "../../api/hooks/experience";
import {
  Button,
  EmptyState,
  InlineMessage,
  QueryBoundary,
  Skeleton,
  Stack,
  Text,
  Visible,
} from "../../components";
import { experienceRequest, type ExperienceView } from "./experienceParams";
import { PastOperationCard } from "./PastOperationCard";
import styles from "./experience.module.css";

/** How many matches the hub returns for a search. */
const BEST = 10;

/**
 * The results of the owner's search. A host search needs a selected host and asks for one
 * otherwise; searching every host is a deliberate choice and sends no host.
 */
export function ExperienceResults({
  view,
  host,
  onGlobal,
}: {
  host: string | undefined;
  onGlobal: () => void;
  view: ExperienceView;
}) {
  const request = experienceRequest(view, host);
  const query = useExperienceSearch(
    request === undefined ? undefined : { ...request, q: request.q.trim() },
  );
  const blank = view.q.trim() === "";
  return (
    <Visible category="P0" id="experience.results">
      {request === undefined ? (
        <EmptyState
          icon="search"
          title="Choose a host first"
          action={<Button onClick={onGlobal}>Search all hosts</Button>}
        >
          <p>
            Experience is searched on one host at a time. Pick a host in the selector, or search all
            hosts on purpose.
          </p>
        </EmptyState>
      ) : (
        <Stack gap={3}>
          {view.scope === "global" && (
            <InlineMessage tone="warning" title="Searching every host">
              Results can come from any host.
            </InlineMessage>
          )}
          <QueryBoundary
            query={query}
            context="search past operations"
            loading={<Skeleton shape="rows" count={3} />}
            isEmpty={(data) => data.items.length === 0}
            empty={
              <EmptyState
                icon="search"
                title={blank ? "No finished operations yet" : "No past operations match"}
              >
                {!blank && <p>Try fewer or different words, or search all hosts.</p>}
              </EmptyState>
            }
          >
            {(data) => (
              <>
                <ul className={styles.list}>
                  {data.items.map((item) => (
                    <li key={item.operation.operation_id}>
                      <PastOperationCard
                        operation={item.operation}
                        snippet={item.snippet}
                        matchedAnyWord={item.match_pass === "any_word"}
                      />
                    </li>
                  ))}
                </ul>
                {data.items.length >= BEST && (
                  <Text muted size="sm">
                    Showing the {BEST} best matches. Narrow the search to see others.
                  </Text>
                )}
              </>
            )}
          </QueryBoundary>
        </Stack>
      )}
    </Visible>
  );
}
