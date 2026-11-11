import { useState } from "react";
import { useAckRules } from "../../api/hooks/rules";
import type { AckRule } from "../../api/types";
import { useSelectedHost } from "../../app/HostContext";
import {
  Button,
  EmptyState,
  InfoTip,
  InlineMessage,
  PageHeader,
  QueryBoundary,
  Section,
  Skeleton,
  Stack,
} from "../../components";
import { AddRuleForm } from "./AddRuleForm";
import { RulesTable } from "./RulesTable";

/** A rule for every host applies to the selected one too. */
function appliesTo(rule: AckRule, host: string | undefined): boolean {
  return host === undefined || !rule.host || rule.host === host;
}

/** Known conditions: list, add and delete them, for the host the selector names. */
export function RulesPage() {
  const rules = useAckRules();
  const [selected] = useSelectedHost();
  const [adding, setAdding] = useState(false);
  const add = () => setAdding(true);

  return (
    <>
      <PageHeader
        title="Known conditions"
        docs="concepts/incidents.html#known-conditions"
        intro={
          <>
            Conditions you expect, so matching incidents stay out of your way.{" "}
            <InfoTip title="Known conditions">
              Incidents that match are kept in the history and never notify. Triage skips them too.
            </InfoTip>
          </>
        }
        actions={
          <Button variant="primary" icon="plus" onClick={add}>
            Add known condition
          </Button>
        }
      />
      <Stack gap={6}>
        <Section
          title="Conditions"
          description="Each one covers the incidents of one check, on one host or on every host."
        >
          <QueryBoundary
            query={rules}
            context="load the known conditions"
            loading={<Skeleton shape="rows" count={4} />}
          >
            {(data) => {
              const items = data.filter((rule) => appliesTo(rule, selected));
              const hidden = data.length - items.length;
              if (items.length === 0) {
                return (
                  <EmptyState
                    icon="rules"
                    title={
                      selected && hidden > 0
                        ? `No known conditions for ${selected}`
                        : "No known conditions yet"
                    }
                    action={
                      <Button variant="primary" icon="plus" onClick={add}>
                        Add known condition
                      </Button>
                    }
                  >
                    <p>
                      {selected && hidden > 0
                        ? "Conditions for other hosts exist. Choose All hosts to see them, or add one for this host."
                        : "Every matching incident notifies you. Add a condition for something you expect, and matching incidents stay in the history without notifying you."}
                    </p>
                  </EmptyState>
                );
              }
              return (
                <Stack gap={3}>
                  {selected !== undefined && hidden > 0 && (
                    <InlineMessage tone="info">
                      {`Showing the known conditions that apply to ${selected}. Choose All hosts to see the others.`}
                    </InlineMessage>
                  )}
                  <RulesTable rules={items} />
                </Stack>
              );
            }}
          </QueryBoundary>
        </Section>
        {/*
          Maintenance windows: add a second <Section title="Maintenance windows"> here, beside
          the conditions above. The page's layout, header actions and host filter need no change.
        */}
      </Stack>
      {adding && <AddRuleForm defaultHost={selected} onClose={() => setAdding(false)} />}
    </>
  );
}
