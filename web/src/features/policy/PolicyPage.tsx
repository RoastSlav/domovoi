import { useSearchParams } from "react-router";
import { usePolicyOverview } from "../../api/hooks/policy";
import type { PolicyOverview } from "../../api/types";
import {
  DetailsScope,
  Grid,
  InfoTip,
  LinkButton,
  PageHeader,
  QueryBoundary,
  Skeleton,
  Stack,
  Tabs,
  Visible,
  VisibilityPage,
  type Tab,
} from "../../components";
import { ActiveRules } from "./ActiveRules";
import { ActiveSummary } from "./ActiveSummary";
import { DecisionsTab } from "./DecisionsTab";
import { DraftCard, DraftStale } from "./DraftCard";
import { PolicyNotes } from "./PolicyNotes";
import { PRECEDENCE_HELP } from "./policyText";
import { ShadowCard } from "./ShadowCard";
import { StartDraftButton } from "./StartDraft";
import { VersionsTab } from "./VersionsTab";
import styles from "./policy.module.css";

const TABS = ["rules", "versions", "decisions"] as const;
type TabId = (typeof TABS)[number];

function Actions({ overview }: { overview: PolicyOverview | undefined }) {
  if (!overview) return null;
  return (
    <>
      {overview.draft ? (
        <LinkButton variant="primary" to="/policy/draft">
          Edit draft
        </LinkButton>
      ) : (
        <StartDraftButton
          variant="primary"
          label="Start a draft"
          from={overview.active?.number ?? null}
          hint="Copies the active version's rules into an editable draft."
        />
      )}
      <LinkButton to="/policy/simulator">Simulator</LinkButton>
    </>
  );
}

function viewTabs(data: PolicyOverview): Tab[] {
  const activeNumber = data.active?.number;
  return [
    {
      id: "rules",
      label: "Rules",
      content: <ActiveRules number={activeNumber} zone={data.time_window_zone} />,
    },
    { id: "versions", label: "Versions", content: <VersionsTab activeNumber={activeNumber} /> },
    {
      id: "decisions",
      label: "Decisions",
      content: <DecisionsTab recording={data.shadow_enabled} />,
    },
  ];
}

/**
 * What Domovoi may do without asking, what it asks about and what it never offers: the active
 * version, the draft, the shadow recorder, and tabs for the rules, versions and recorded
 * decisions.
 */
export function PolicyPage() {
  const overview = usePolicyOverview();
  const [params, setParams] = useSearchParams();
  const asked = params.get("tab");
  const tab: TabId = TABS.find((id) => id === asked) ?? "rules";
  const select = (id: string) => {
    const next = new URLSearchParams(params);
    if (id === "rules") next.delete("tab");
    else next.set("tab", id);
    setParams(next, { replace: true });
  };
  return (
    <VisibilityPage page="policy">
      <PageHeader
        title="Policy"
        docs="concepts/policy.html"
        intro={
          <>
            What Domovoi may do without asking you, what it asks you about and what it never offers.{" "}
            <InfoTip title="How rules combine" learnMore="concepts/policy.html">
              {PRECEDENCE_HELP}
            </InfoTip>
          </>
        }
        actions={<Actions overview={overview.data} />}
      />
      <Visible category="P0" id="policy.state">
        <QueryBoundary
          query={overview}
          context="load the policy"
          loading={<Skeleton shape="rows" count={4} />}
        >
          {(data) => (
            <Stack gap={4}>
              <DraftStale overview={data} />
              <Grid min="sm" gap={4}>
                <Visible category="P0" id="policy.active">
                  <ActiveSummary version={data.active} zone={data.time_window_zone} />
                </Visible>
                <DraftCard overview={data} />
              </Grid>
              <Visible category="P0" id="policy.shadow">
                <ShadowCard overview={data} />
              </Visible>
              <Visible category="P0" id="policy.mode">
                <PolicyNotes notes={data.notes} />
              </Visible>
              {data.active && (
                <DetailsScope>
                  <Visible category="P3" id="policy.fingerprint" label="Fingerprint" count={1}>
                    <code className={styles.fingerprint}>{data.active.rules_hash}</code>
                  </Visible>
                </DetailsScope>
              )}
              <Tabs label="Policy views" tabs={viewTabs(data)} selected={tab} onSelect={select} />
            </Stack>
          )}
        </QueryBoundary>
      </Visible>
    </VisibilityPage>
  );
}
