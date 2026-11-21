import type { PolicyDraft } from "../../api/types";
import { DetailsScope, KeyValueList, RelativeTime, Text, Visible } from "../../components";
import { actorText } from "./ruleText";

/**
 * The stored draft's details: revision, who saved it and when, and the rules fingerprint.
 * @param draft The draft as stored.
 */
export function DraftDetails({ draft }: { draft: PolicyDraft }) {
  return (
    <DetailsScope>
      <Visible category="P3" id="draft.metadata" count={3} label="Draft details">
        <KeyValueList
          items={[
            { label: "Revision", value: draft.revision },
            {
              label: "Last saved",
              value: (
                <>
                  {`by ${actorText(draft.updated_by)} `}
                  <RelativeTime at={draft.updated_at} />
                </>
              ),
            },
            { label: "Rules fingerprint", value: <Text mono>{draft.rules_hash}</Text> },
          ]}
        />
      </Visible>
    </DetailsScope>
  );
}
