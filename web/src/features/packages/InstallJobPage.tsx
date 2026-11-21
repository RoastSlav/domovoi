import { useParams } from "react-router";
import { useInstallJob } from "../../api/hooks/packageJobs";
import { ApiFailure, type InstallJob } from "../../api/types";
import { AllHostsNote } from "../../app/AllHostsNote";
import { installJobStatusMeta } from "../../design/tone";
import {
  DetailsScope,
  EmptyState,
  KeyValueList,
  LinkButton,
  PageHeader,
  QueryBoundary,
  RelativeTime,
  Skeleton,
  StatusPill,
  Stack,
  Text,
  Visible,
  VisibilityPage,
} from "../../components";
import { JobTargets } from "./JobTargets";
import { planCode } from "./jobModel";

const BACK = "/capabilities?tab=packages";

/** A requested-by value worth showing: a name, not an identifier. */
function readable(by: string): boolean {
  return by !== "" && !/^[0-9a-f-]{24,}$/i.test(by);
}

/** The plan code, approval window and requester: metadata, never needed to act. */
function Details({ job }: { job: InstallJob }) {
  const expires = job.targets
    .map((target) => target.expires_at)
    .sort()
    .pop();
  return (
    <DetailsScope>
      <Visible category="P3" id="job.details" label="Details">
        <KeyValueList
          items={[
            { label: "Plan", value: <Text mono>{planCode(job.job_plan_hash)}</Text> },
            ...(expires
              ? [{ label: "Approval window ends", value: <RelativeTime at={expires} /> }]
              : []),
            ...(readable(job.requested_by)
              ? [{ label: "Requested by", value: job.requested_by }]
              : []),
          ]}
        />
      </Visible>
    </DetailsScope>
  );
}

/**
 * One install job: each host's current attempt with what it is doing or why it stopped, and
 * the actions that fit (retry, cancel, check the host). It refreshes every few seconds while
 * a host is still working.
 */
export function InstallJobPage() {
  const { id = "" } = useParams();
  const query = useInstallJob(id);
  const job = query.data;
  const gone =
    job === undefined &&
    query.error instanceof ApiFailure &&
    (query.error.code === "no_such_job" || query.error.status === 404);
  return (
    <VisibilityPage page="capabilities">
      <PageHeader
        title={
          job
            ? `${job.kind === "uninstall" ? "Remove" : "Install"} ${job.name} ${job.version}`
            : "Install job"
        }
        meta={
          job && (
            <Visible category="P0" id="job.status">
              <StatusPill meta={installJobStatusMeta(job.status)} />
            </Visible>
          )
        }
        intro={
          job ? (
            <>
              Requested <RelativeTime at={job.created_at} />. This page refreshes every few seconds
              while a host is still working.
            </>
          ) : undefined
        }
        actions={<LinkButton to={BACK}>Back to packages</LinkButton>}
      />
      <AllHostsNote>
        An install can reach several hosts, so this page shows all of them.
      </AllHostsNote>
      {gone ? (
        <EmptyState
          icon="package"
          title="No such install"
          action={<LinkButton to={BACK}>Back to packages</LinkButton>}
        >
          That install no longer exists.
        </EmptyState>
      ) : (
        <QueryBoundary query={query} context="load the install" loading={<Skeleton shape="rows" />}>
          {(data) => (
            <Stack gap={5}>
              {query.error && (
                <Text muted block>
                  Could not refresh. Trying again.
                </Text>
              )}
              <JobTargets job={data} />
              <Details job={data} />
            </Stack>
          )}
        </QueryBoundary>
      )}
    </VisibilityPage>
  );
}
