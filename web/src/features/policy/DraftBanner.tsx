import { Link } from "react-router";
import { Button, InlineMessage, Stack, Text, Visible } from "../../components";
import { issuesByRule, type RuleIssue } from "./ruleIssues";

/**
 * What the person must know before saving: unsaved edits, a draft changed elsewhere, and the
 * hub's problems with the rules of a failed check or save. Each says it in words.
 * @param dirty There are unsaved edits.
 * @param changes How many rules differ from the stored draft.
 * @param elsewhere The stored draft moved on while edits were open.
 * @param issues The hub's problems, with `rules.<id>` paths.
 * @param onReload Starts the reload, which asks first.
 */
export function DraftBanner({
  dirty,
  changes,
  elsewhere,
  issues,
  onReload,
}: {
  changes: number;
  dirty: boolean;
  elsewhere: boolean;
  issues: readonly RuleIssue[];
  onReload: () => void;
}) {
  if (!dirty && !elsewhere && issues.length === 0) return null;
  const { byRule, rest } = issuesByRule(issues);
  return (
    <Visible category="P0" id="draft.banner">
      <Stack gap={3}>
        {elsewhere && (
          <InlineMessage
            tone="warning"
            title="This draft changed elsewhere"
            action={
              <Button size="sm" onClick={onReload}>
                Reload the draft
              </Button>
            }
          >
            Saving is blocked until you reload. Reloading discards the edits on this page.
          </InlineMessage>
        )}
        {issues.length > 0 && (
          <InlineMessage tone="danger" title="The hub did not accept these rules">
            <ul>
              {[...byRule].map(([id, messages]) => (
                <li key={id}>
                  <Link to={`?rule=${encodeURIComponent(id)}`}>{id}</Link>
                  {`: ${messages.join(" ")}`}
                </li>
              ))}
              {rest.map((message) => (
                <li key={message}>{message}</li>
              ))}
            </ul>
          </InlineMessage>
        )}
        {dirty && (
          <InlineMessage tone="info" title="You have unsaved changes">
            <Text>
              {changes === 1
                ? "1 rule differs from the saved draft. "
                : `${changes} rules differ from the saved draft. `}
              Nothing is stored until you save the draft.
            </Text>
          </InlineMessage>
        )}
      </Stack>
    </Visible>
  );
}
