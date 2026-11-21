import { useState } from "react";
import { useSearchParams } from "react-router";
import { useQueryClient } from "@tanstack/react-query";
import { AllHostsNote } from "../../app/AllHostsNote";
import { useAuditVerification, type AuditHistoryName } from "../../api/hooks/audit";
import { keys } from "../../api/queryClient";
import {
  Button,
  InlineMessage,
  PageHeader,
  QueryBoundary,
  Section,
  Skeleton,
  Stack,
  Tabs,
  Visible,
  VisibilityPage,
} from "../../components";
import { AnchorDetails } from "./AnchorDetails";
import { AuditTable } from "./AuditTable";
import styles from "./audit.module.css";
import { VerificationCards } from "./VerificationCards";

/** The audit log: what Domovoi recorded, and whether the record still holds. */
export function AuditPage() {
  const [params, setParams] = useSearchParams();
  const tab: AuditHistoryName = params.get("tab") === "legacy" ? "legacy" : "chained";
  const [fullAsked, setFullAsked] = useState(false);
  const recent = useAuditVerification("recent");
  const full = useAuditVerification("full", fullAsked);
  const client = useQueryClient();
  const check = full.data ?? recent.data;
  const firstBroken =
    check?.chained.status === "broken" ? (check.chained.first_broken_seq ?? undefined) : undefined;
  return (
    <VisibilityPage page="audit">
      <PageHeader
        title="Audit log"
        intro="What Domovoi recorded, and whether the record still holds."
        docs="guide/audit.html"
      />
      <AllHostsNote>The audit log belongs to the hub, not to one host.</AllHostsNote>
      <Stack gap={6}>
        <Section title="History check">
          <Visible category="P0" id="audit.verification">
            <QueryBoundary
              query={recent}
              context="check the audit log"
              loading={<Skeleton shape="block" />}
            >
              {(data) => (
                <VerificationCards
                  recent={data}
                  full={full}
                  fullAsked={fullAsked}
                  onCheckFull={() => setFullAsked(true)}
                />
              )}
            </QueryBoundary>
            {recent.error && !recent.data && (
              <p className={styles.muted}>
                If this keeps failing, run domovoi audit verify on the hub&apos;s host.
              </p>
            )}
          </Visible>
        </Section>
        <Visible category="P0" id="audit.limits">
          <InlineMessage tone="info" title="What this check can show">
            A broken chain shows that entries were edited, removed or added outside Domovoi. It
            cannot show that someone rewrote the whole database and recomputed every hash, or that
            the newest entries were removed, unless you kept the head hash elsewhere.
          </InlineMessage>
        </Visible>
        {check && (
          <Visible category="P2" id="audit.anchor" label="Anchor details" count={5}>
            <AnchorDetails anchor={check.anchor} />
          </Visible>
        )}
        <Section
          title="Entries"
          actions={
            <Visible category="P1" id="audit.refresh">
              <Button
                variant="ghost"
                icon="refresh"
                onClick={() => void client.refetchQueries({ queryKey: keys.auditRows(tab) })}
              >
                Refresh
              </Button>
            </Visible>
          }
        >
          <Visible category="P0" id="audit.entries">
            <Tabs
              label="Audit history"
              selected={tab}
              onSelect={(id) =>
                setParams(id === "legacy" ? { tab: "legacy" } : {}, { replace: true })
              }
              tabs={[
                {
                  id: "chained",
                  label: "Chained history",
                  content: <AuditTable history="chained" firstBroken={firstBroken} />,
                },
                {
                  id: "legacy",
                  label: "Legacy history",
                  content: <AuditTable history="legacy" firstBroken={undefined} />,
                },
              ]}
            />
          </Visible>
        </Section>
      </Stack>
    </VisibilityPage>
  );
}
