import { Button, Cluster, LinkButton, Menu, Visible } from "../../components";

/**
 * The page's actions: add a rule, check the rules, review and publish, and More. Review and
 * publish is the one primary action only while there is nothing unsaved; with edits, saving
 * is the primary action (the floating bar) and publishing waits.
 * @param dirty There are unsaved edits.
 * @param checking A check is running.
 * @param onCheck Starts a check of the rules on this page.
 * @param onDiscard Opens the question about discarding the stored draft.
 */
export function DraftActions({
  dirty,
  checking,
  onCheck,
  onDiscard,
}: {
  checking: boolean;
  dirty: boolean;
  onCheck: () => void;
  onDiscard: () => void;
}) {
  return (
    <Visible category="P1" id="draft.actions">
      <Cluster>
        <LinkButton to="?rule=new" icon="plus">
          Add rule
        </LinkButton>
        <Button
          loading={checking}
          hint="Checking changes nothing. Saving and publishing are separate steps."
          onClick={onCheck}
        >
          Check rules
        </Button>
        {dirty ? (
          <Button disabledReason="Save the draft first.">Review and publish</Button>
        ) : (
          <LinkButton to="/policy/draft/publish" variant="primary">
            Review and publish
          </LinkButton>
        )}
        <Menu
          text="More"
          items={[
            { label: "Discard draft...", icon: "trash", tone: "danger", onSelect: onDiscard },
          ]}
        />
      </Cluster>
    </Visible>
  );
}
