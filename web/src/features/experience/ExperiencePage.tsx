import { useEffect, useState } from "react";
import { useSearchParams } from "react-router";
import { useSelectedHost } from "../../app/HostContext";
import {
  InlineMessage,
  PageHeader,
  Stack,
  Visible,
  VisibilityPage,
  useDebounced,
} from "../../components";
import { HISTORICAL_LABEL } from "../../design/knowledgeText";
import { readExperienceView, writeExperienceView, type ExperienceScope } from "./experienceParams";
import { ExperienceFilters } from "./ExperienceFilters";
import { ExperienceResults } from "./ExperienceResults";

/** How long typing must pause before a search runs. */
const SEARCH_DEBOUNCE_MS = 300;

const NOTICE_TITLE = HISTORICAL_LABEL.charAt(0).toUpperCase() + HISTORICAL_LABEL.slice(1);

/**
 * Owner search over finished operations: one host by default, every host only when asked.
 * The scope and the words live in the address. Past operations are evidence, never permission.
 */
export function ExperiencePage() {
  const [params, setParams] = useSearchParams();
  const [host] = useSelectedHost();
  const view = readExperienceView(params);
  const [text, setText] = useState(view.q);
  const settled = useDebounced(text.trim(), SEARCH_DEBOUNCE_MS);

  useEffect(() => {
    if (settled !== readExperienceView(params).q) {
      setParams((previous) => writeExperienceView(previous, { q: settled }), { replace: true });
    }
  }, [settled, params, setParams]);

  const setScope = (scope: ExperienceScope) =>
    setParams((previous) => writeExperienceView(previous, { scope }), { replace: true });

  return (
    <VisibilityPage page="experience">
      <PageHeader
        title="Experience"
        intro="Past operations you can search. They are evidence, never permission."
        docs="concepts/experience.html"
      />
      <Stack gap={4}>
        <Visible category="P0" id="experience.label">
          <InlineMessage tone="info" title={NOTICE_TITLE}>
            A past approval or success never lets anything run now. Every action still needs its own
            approval.
          </InlineMessage>
        </Visible>
        <ExperienceFilters scope={view.scope} text={text} onScope={setScope} onText={setText} />
        <ExperienceResults view={view} host={host} onGlobal={() => setScope("global")} />
      </Stack>
    </VisibilityPage>
  );
}
