import { Link } from "react-router";
import { Highlight } from "../../../components";
import { ResultGroup } from "../ResultGroup";
import { SEARCH_GROUPS, type SearchGroups } from "../useSearchGroups";

/** Notes the hub found for the text, with the matching excerpt. */
export function NotesGroup({ q, state }: { q: string; state: SearchGroups["notes"] }) {
  return (
    <ResultGroup
      group={SEARCH_GROUPS[3]}
      state={state}
      rows={state.items.map((hit) => ({
        key: hit.note.id,
        node: (
          <>
            <Link to={`/notes/${encodeURIComponent(hit.note.id)}`}>
              <Highlight text={hit.note.title} query={q} />
            </Link>
            {hit.snippet && <span>{hit.snippet}</span>}
          </>
        ),
      }))}
    />
  );
}
