import { AllHostsNote } from "../../app/AllHostsNote";
import {
  EmptyState,
  InlineMessage,
  PageHeader,
  Stack,
  TextInput,
  Visible,
  VisibilityPage,
} from "../../components";
import { ActiveOperationsGroup } from "./groups/ActiveOperationsGroup";
import { ExpectedGroup } from "./groups/ExpectedGroup";
import { FactsGroup } from "./groups/FactsGroup";
import { HostsGroup } from "./groups/HostsGroup";
import { KnownConditionsGroup } from "./groups/KnownConditionsGroup";
import { IncidentsGroup } from "./groups/IncidentsGroup";
import { NotesGroup } from "./groups/NotesGroup";
import { PastOperationsGroup } from "./groups/PastOperationsGroup";
import styles from "./search.module.css";
import { MIN_QUERY, SEARCH_GROUPS, scopeSentence, useSearchGroups } from "./useSearchGroups";
import { useSearchQuery } from "./useSearchQuery";

/** One search over incidents, operations, notes, facts and hosts, whichever host is selected. */
export function SearchPage() {
  const { q, text, setText } = useSearchQuery();
  const groups = useSearchGroups(q);
  const asked = q.length >= MIN_QUERY;
  const states = [
    groups.incidents,
    groups.operations,
    groups.past,
    groups.notes,
    groups.facts,
    groups.hosts,
    groups.expected,
  ];
  const settled = asked && states.every((s) => s.status === "ready" || s.status === "error");
  const failed = states.some((s) => s.status === "error");
  const nothing = states.every((s) => s.status === "ready" && s.total === 0);
  return (
    <VisibilityPage page="search">
      <PageHeader
        title="Search"
        intro="Find incidents, operations, notes and facts across every host."
      />
      <AllHostsNote>Search covers every host.</AllHostsNote>
      <Stack gap={6}>
        <div role="search" className={styles.box}>
          <Visible category="P1" id="search.box">
            <TextInput
              type="search"
              aria-label="Search"
              maxLength={200}
              placeholder="Search incidents, operations, notes and facts"
              value={text}
              onChange={(event) => setText(event.target.value)}
            />
          </Visible>
        </div>
        <Visible category="P0" id="search.scope">
          <p className={styles.muted}>{scopeSentence(SEARCH_GROUPS)}</p>
        </Visible>
        {!asked && <p className={styles.muted}>Type at least 2 characters.</p>}
        {asked && (
          <>
            <IncidentsGroup q={q} state={groups.incidents} />
            <ActiveOperationsGroup q={q} state={groups.operations} />
            <PastOperationsGroup q={q} state={groups.past} label={groups.historyLabel} />
            <NotesGroup q={q} state={groups.notes} />
            <FactsGroup state={groups.facts} />
            <HostsGroup q={q} state={groups.hosts} />
            <KnownConditionsGroup q={q} state={groups.rules} />
            <ExpectedGroup q={q} state={groups.expected} />
            <Visible category="P0" id="search.status">
              {settled && failed && (
                <InlineMessage tone="warning" title="Some sources could not be searched">
                  Results from the others are shown.
                </InlineMessage>
              )}
              {settled && nothing && (
                <EmptyState icon="search" title="Nothing found">
                  Nothing matches “{q}” in the sources above.
                </EmptyState>
              )}
            </Visible>
          </>
        )}
      </Stack>
    </VisibilityPage>
  );
}
