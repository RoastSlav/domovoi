import { Link, useParams } from "react-router";
import { useHostNames } from "../../api/hooks/hosts";
import { usePolicyOverview, usePolicyVersion } from "../../api/hooks/policy";
import { ApiFailure, type PolicyVersionDetail } from "../../api/types";
import {
  Cluster,
  DetailsScope,
  EmptyState,
  KeyValueList,
  LinkButton,
  LoadError,
  PageHeader,
  QueryBoundary,
  RelativeTime,
  Skeleton,
  Stack,
  Tag,
  Text,
  Visible,
  VisibilityPage,
} from "../../components";
import { ruleMarkMeta } from "../../design/tone";
import { VersionAcks } from "./VersionAcks";
import { RulesTable } from "./RulesTable";
import { actorText } from "./ruleText";
import { StartDraftButton } from "./StartDraft";

function Missing() {
  return (
    <EmptyState
      icon="shield"
      title="No such version"
      action={<Link to="/policy">Back to Policy</Link>}
    >
      This version was never published.
    </EmptyState>
  );
}

function Body({ detail, zone }: { detail: PolicyVersionDetail; zone: string | undefined }) {
  const hostNames = useHostNames();
  const v = detail.summary;
  return (
    <Stack gap={4}>
      <Visible category="P1" id="version.summary">
        <KeyValueList
          items={[
            {
              label: "Published",
              value: (
                <>
                  by {actorText(v.published_by)} <RelativeTime at={v.published_at} />
                </>
              ),
            },
            { label: "Rules", value: v.rule_count },
            { label: "Locked", value: v.locked_count },
            { label: ruleMarkMeta("shadow").label, value: v.shadow_count },
            { label: "Time zone", value: zone ?? "The hub did not say" },
          ]}
        />
      </Visible>
      <Visible category="P1" id="version.rules">
        <RulesTable rules={detail.rules} hostNames={hostNames} zone={zone} />
      </Visible>
      <Visible category="P0" id="version.acknowledgements">
        <VersionAcks items={detail.acknowledgements} />
      </Visible>
      <DetailsScope>
        <Visible category="P3" id="policy.fingerprint" label="Fingerprint" count={1}>
          <Text mono>{v.rules_hash}</Text>
        </Visible>
      </DetailsScope>
    </Stack>
  );
}

/**
 * One published version: its rules, the conflicts that were acknowledged when it was
 * published, and whether it is still the active one.
 */
export function VersionPage() {
  const { number: raw } = useParams();
  const number = raw !== undefined && /^\d+$/.test(raw) ? Number(raw) : undefined;
  const version = usePolicyVersion(number);
  const overview = usePolicyOverview();
  const active = overview.data?.active?.number;
  const draftExists = Boolean(overview.data?.draft);
  const notFound = version.error instanceof ApiFailure && version.error.status === 404;
  return (
    <VisibilityPage page="policy">
      <PageHeader
        title={number === undefined ? "Version" : `Version ${number}`}
        docs="concepts/policy.html"
        topic="policy versions"
        meta={
          number !== undefined &&
          active !== undefined && (
            <Visible category="P0" id="version.status">
              {active === number ? (
                <Tag tone="success">Active</Tag>
              ) : (
                <Tag>Replaced by version {active}</Tag>
              )}
            </Visible>
          )
        }
        actions={
          number === undefined ? null : (
            <Cluster gap={2}>
              <StartDraftButton
                from={number}
                label="Start a draft from this version"
                disabledReason={
                  draftExists ? "A draft already exists. Edit or discard it first." : undefined
                }
              />
              <LinkButton to={`/policy/simulator?rules=version-${number}`}>Simulate</LinkButton>
            </Cluster>
          )
        }
      />
      <Visible category="P0" id="policy.state">
        {number === undefined || notFound ? (
          <Missing />
        ) : version.error && version.data === undefined ? (
          <LoadError error={version.error} context="load the version" onRetry={version.refetch} />
        ) : (
          <QueryBoundary
            query={version}
            context="load the version"
            loading={<Skeleton shape="rows" count={4} />}
          >
            {(detail) => <Body detail={detail} zone={overview.data?.time_window_zone} />}
          </QueryBoundary>
        )}
      </Visible>
    </VisibilityPage>
  );
}
